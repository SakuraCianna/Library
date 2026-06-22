use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use std::{env, fs};

use crate::error::AppError;
use crate::models::{OcrEnvironmentReport, OcrSidecarRequest, OcrSidecarResult, ParsedDocument};

const SUMMARY_CHARS: usize = 180;
const MAX_OCR_BODY_CHARS: usize = 60_000;
const OCR_SIDECAR_TIMEOUT: Duration = Duration::from_secs(120);
const OCR_ENV_CHECK_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_OCR_INPUT_BYTES: u64 = 50 * 1024 * 1024;
const DEFAULT_MAX_OCR_PDF_PAGES: u32 = 12;
const REQUIRED_MODEL_FILES: [&str; 3] = ["inference.json", "inference.pdiparams", "inference.yml"];

#[derive(Debug, serde::Deserialize)]
struct OcrSidecarEnvelope {
    ok: bool,
    result: Option<OcrSidecarResult>,
    error: Option<OcrSidecarError>,
}

#[derive(Debug, serde::Deserialize)]
struct OcrSidecarError {
    code: String,
    message: String,
}

pub fn build_ocr_request(file_path: &Path, model_dir: &Path, tier: &str) -> OcrSidecarRequest {
    OcrSidecarRequest {
        file_path: file_path.to_string_lossy().to_string(),
        model_dir: model_dir.to_string_lossy().to_string(),
        tier: tier.to_string(),
        max_pdf_pages: ocr_pdf_page_limit(),
    }
}

pub fn run_ocr_sidecar_cancellable<F>(
    request: &OcrSidecarRequest,
    resource_script_path: Option<&Path>,
    is_cancelled: F,
) -> Result<OcrSidecarResult, AppError>
where
    F: Fn() -> bool,
{
    let script_path = resolve_ocr_sidecar_script(resource_script_path)?;
    let project_root = discover_project_root().ok();
    let python_path = discover_python_executable(project_root.as_deref());
    run_ocr_sidecar_with_paths(
        request,
        &python_path,
        &script_path,
        OCR_SIDECAR_TIMEOUT,
        is_cancelled,
    )
}

pub fn check_ocr_environment(
    app_data_dir: &Path,
    resource_checker_path: Option<&Path>,
) -> Result<OcrEnvironmentReport, AppError> {
    let checker_path = resolve_ocr_environment_checker(resource_checker_path)?;
    let project_root = discover_project_root().ok();
    let python_path = discover_python_executable(project_root.as_deref());
    let config = crate::runtime::ocr_config(app_data_dir);

    run_ocr_environment_check_with_paths(
        &python_path,
        &checker_path,
        &config.model_dir,
        &config.tier,
        OCR_ENV_CHECK_TIMEOUT,
    )
}

fn run_ocr_sidecar_with_paths<F>(
    request: &OcrSidecarRequest,
    python_path: &Path,
    script_path: &Path,
    timeout: Duration,
    is_cancelled: F,
) -> Result<OcrSidecarResult, AppError>
where
    F: Fn() -> bool,
{
    if !script_path.is_file() {
        return Err(AppError::Filesystem(format!(
            "找不到 OCR sidecar：{}",
            script_path.display()
        )));
    }

    let mut child = Command::new(python_path)
        .arg(script_path)
        .env("DISABLE_MODEL_SOURCE_CHECK", "True")
        .env("PADDLE_PDX_DISABLE_MODEL_SOURCE_CHECK", "True")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| AppError::Filesystem(format!("无法启动 OCR sidecar：{error}")))?;

    let stdout_handle = child
        .stdout
        .take()
        .ok_or_else(|| AppError::Filesystem("无法读取 OCR sidecar stdout".to_string()))
        .map(read_output_pipe)?;
    let stderr_handle = child
        .stderr
        .take()
        .ok_or_else(|| AppError::Filesystem("无法读取 OCR sidecar stderr".to_string()))
        .map(read_output_pipe)?;

    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| AppError::Filesystem("无法写入 OCR sidecar stdin".to_string()))?;
        let payload = serde_json::to_vec(request)
            .map_err(|error| AppError::Filesystem(format!("无法序列化 OCR 请求：{error}")))?;
        stdin
            .write_all(&payload)
            .map_err(|error| AppError::Filesystem(format!("无法发送 OCR 请求：{error}")))?;
    }

    let start = Instant::now();
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| AppError::Filesystem(format!("无法等待 OCR sidecar：{error}")))?
        {
            break status;
        }
        if is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AppError::Filesystem(
                "OCR_CANCELLED：用户取消了 OCR 任务".to_string(),
            ));
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AppError::Filesystem(
                "OCR_TIMEOUT：OCR sidecar 执行超时".to_string(),
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    };

    let stdout = join_output_reader(stdout_handle, "OCR 输出")?;
    let stderr = join_output_reader(stderr_handle, "OCR 日志")?;

    if !status.success() {
        return Err(AppError::Filesystem(format!(
            "OCR sidecar 退出失败：{}",
            truncate_chars(stderr.trim(), 500)
        )));
    }

    parse_ocr_sidecar_stdout(&stdout)
}

fn run_ocr_environment_check_with_paths(
    python_path: &Path,
    checker_path: &Path,
    model_dir: &Path,
    tier: &str,
    timeout: Duration,
) -> Result<OcrEnvironmentReport, AppError> {
    if !checker_path.is_file() {
        return Err(AppError::Filesystem(format!(
            "找不到 OCR 环境自检脚本：{}",
            checker_path.display()
        )));
    }

    let mut child = Command::new(python_path)
        .arg(checker_path)
        .arg("--model-dir")
        .arg(model_dir)
        .arg("--tier")
        .arg(tier)
        .arg("--require-runtime")
        .arg("--json")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| AppError::Filesystem(format!("无法启动 OCR 环境自检：{error}")))?;

    let stdout_handle = child
        .stdout
        .take()
        .ok_or_else(|| AppError::Filesystem("无法读取 OCR 环境自检 stdout".to_string()))
        .map(read_output_pipe)?;
    let stderr_handle = child
        .stderr
        .take()
        .ok_or_else(|| AppError::Filesystem("无法读取 OCR 环境自检 stderr".to_string()))
        .map(read_output_pipe)?;

    let start = Instant::now();
    loop {
        if child
            .try_wait()
            .map_err(|error| AppError::Filesystem(format!("无法等待 OCR 环境自检：{error}")))?
            .is_some()
        {
            break;
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AppError::Filesystem(
                "OCR_ENV_CHECK_TIMEOUT：OCR 环境自检超时".to_string(),
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let stdout = join_output_reader(stdout_handle, "OCR 环境自检输出")?;
    let stderr = join_output_reader(stderr_handle, "OCR 环境自检日志")?;

    parse_ocr_environment_report(&stdout).map_err(|error| {
        AppError::Filesystem(format!(
            "{}；日志：{}",
            error,
            truncate_chars(stderr.trim(), 500)
        ))
    })
}

fn read_output_pipe<R>(mut reader: R) -> std::thread::JoinHandle<std::io::Result<String>>
where
    R: Read + Send + 'static,
{
    std::thread::spawn(move || {
        let mut output = String::new();
        reader.read_to_string(&mut output)?;
        Ok(output)
    })
}

fn join_output_reader(
    handle: std::thread::JoinHandle<std::io::Result<String>>,
    label: &str,
) -> Result<String, AppError> {
    handle
        .join()
        .map_err(|_| AppError::Filesystem(format!("{label}读取线程异常退出")))?
        .map_err(|error| AppError::Filesystem(format!("无法读取{label}：{error}")))
}

pub fn validate_ocr_inputs(file_path: &Path, model_dir: &Path, tier: &str) -> Result<(), AppError> {
    if !file_path.is_file() {
        return Err(AppError::Filesystem("OCR 输入文件不存在".to_string()));
    }
    let file_size = fs::metadata(file_path)
        .map_err(|error| AppError::Filesystem(format!("无法读取 OCR 输入文件信息：{error}")))?
        .len();
    if file_size > MAX_OCR_INPUT_BYTES {
        return Err(AppError::Filesystem(format!(
            "OCR_INPUT_TOO_LARGE：OCR 输入文件过大，当前上限为 {} MB",
            MAX_OCR_INPUT_BYTES / 1024 / 1024
        )));
    }
    if !model_dir.is_dir() {
        return Err(AppError::Filesystem("OCR 模型目录不存在".to_string()));
    }
    let missing_models = required_ocr_models(tier)
        .into_iter()
        .filter(|model_name| !model_dir.join(model_name).is_dir())
        .collect::<Vec<_>>();
    if !missing_models.is_empty() {
        return Err(AppError::Filesystem(format!(
            "OCR 模型不完整，缺少 {}",
            missing_models.join("、")
        )));
    }
    let missing_files = required_ocr_model_files(tier)
        .into_iter()
        .filter(|relative_path| !model_dir.join(relative_path).is_file())
        .collect::<Vec<_>>();
    if !missing_files.is_empty() {
        return Err(AppError::Filesystem(format!(
            "OCR 模型文件不完整，缺少 {}",
            missing_files.join("、")
        )));
    }

    Ok(())
}

fn ocr_pdf_page_limit() -> u32 {
    env::var("OCR_MAX_PDF_PAGES")
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_OCR_PDF_PAGES)
}

fn required_ocr_models(tier: &str) -> [String; 2] {
    [
        format!("PP-OCRv6_{tier}_det"),
        format!("PP-OCRv6_{tier}_rec"),
    ]
}

fn required_ocr_model_files(tier: &str) -> Vec<String> {
    required_ocr_models(tier)
        .into_iter()
        .flat_map(|model_name| {
            REQUIRED_MODEL_FILES
                .iter()
                .map(move |file_name| format!("{model_name}/{file_name}"))
        })
        .collect()
}

pub fn resolve_ocr_sidecar_script(
    resource_script_path: Option<&Path>,
) -> Result<PathBuf, AppError> {
    if let Some(resource_script_path) = resource_script_path.filter(|path| path.is_file()) {
        return Ok(resource_script_path.to_path_buf());
    }

    if let Ok(explicit_path) = env::var("OCR_SIDECAR_PATH") {
        let trimmed = explicit_path.trim();
        if !trimmed.is_empty() {
            let explicit_path = PathBuf::from(trimmed);
            if explicit_path.is_file() {
                return Ok(explicit_path);
            }
            return Err(AppError::Filesystem(format!(
                "OCR_SIDECAR_PATH 指向的 sidecar 不存在：{}",
                explicit_path.display()
            )));
        }
    }

    let project_root = discover_project_root()?;
    Ok(project_root
        .join("sidecars")
        .join("ocr")
        .join("ocr_sidecar.py"))
}

pub fn resolve_ocr_environment_checker(
    resource_checker_path: Option<&Path>,
) -> Result<PathBuf, AppError> {
    if let Some(resource_checker_path) = resource_checker_path.filter(|path| path.is_file()) {
        return Ok(resource_checker_path.to_path_buf());
    }

    let project_root = discover_project_root()?;
    Ok(project_root
        .join("sidecars")
        .join("ocr")
        .join("check_ocr_environment.py"))
}

fn discover_project_root() -> Result<PathBuf, AppError> {
    let current_dir = env::current_dir()
        .map_err(|error| AppError::Filesystem(format!("无法读取当前目录：{error}")))?;
    current_dir
        .ancestors()
        .find(|path| {
            path.join("sidecars")
                .join("ocr")
                .join("ocr_sidecar.py")
                .is_file()
        })
        .map(Path::to_path_buf)
        .ok_or_else(|| AppError::Filesystem("找不到项目根目录下的 OCR sidecar".to_string()))
}

fn discover_python_executable(project_root: Option<&Path>) -> PathBuf {
    if let Ok(explicit_path) = env::var("OCR_PYTHON_PATH") {
        let trimmed = explicit_path.trim();
        if !trimmed.is_empty() {
            let explicit_path = PathBuf::from(trimmed);
            if explicit_path.is_file() {
                return explicit_path;
            }
        }
    }

    if let Some(project_root) = project_root {
        let local_python = project_root
            .join(".venv")
            .join("Scripts")
            .join("python.exe");
        if local_python.is_file() {
            return local_python;
        }
    }

    PathBuf::from("python")
}

pub fn parse_ocr_sidecar_stdout(stdout: &str) -> Result<OcrSidecarResult, AppError> {
    let envelope: OcrSidecarEnvelope = serde_json::from_str(stdout.trim())
        .map_err(|error| AppError::Filesystem(format!("OCR sidecar 返回了无效 JSON：{error}")))?;

    if envelope.ok {
        return envelope
            .result
            .ok_or_else(|| AppError::Filesystem("OCR sidecar 缺少 result".to_string()));
    }

    let error = envelope
        .error
        .ok_or_else(|| AppError::Filesystem("OCR sidecar 返回失败但缺少错误信息".to_string()))?;
    Err(AppError::Filesystem(format!(
        "{}：{}",
        error.code, error.message
    )))
}

pub fn parse_ocr_environment_report(stdout: &str) -> Result<OcrEnvironmentReport, AppError> {
    serde_json::from_str(stdout.trim())
        .map_err(|error| AppError::Filesystem(format!("OCR 环境自检返回了无效 JSON：{error}")))
}

pub fn build_ocr_document(
    relative_path: &str,
    result: &OcrSidecarResult,
) -> Result<ParsedDocument, AppError> {
    let body = normalize_text(&result.text);
    if body.is_empty() {
        return Err(AppError::Filesystem("OCR 结果为空".to_string()));
    }

    Ok(ParsedDocument {
        title: display_file_name(relative_path),
        summary: truncate_chars(&body, SUMMARY_CHARS),
        body: truncate_chars(&body, MAX_OCR_BODY_CHARS),
        source_locator: format!("{relative_path}#ocr"),
    })
}

fn normalize_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut output = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        output.push('…');
    }
    output
}

fn display_file_name(relative_path: &str) -> String {
    relative_path
        .rsplit(['\\', '/'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(relative_path)
        .to_string()
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, OnceLock};
    use std::{env, fs};

    use super::{
        build_ocr_document, build_ocr_request, parse_ocr_environment_report,
        parse_ocr_sidecar_stdout, resolve_ocr_environment_checker, resolve_ocr_sidecar_script,
        validate_ocr_inputs,
    };

    #[test]
    fn validates_existing_file_and_model_dir() {
        let _env_lock = env_lock().lock().expect("env lock");
        let _page_guard = EnvVarGuard::set("OCR_MAX_PDF_PAGES", "12");
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let input = temp_dir.path().join("scan.pdf");
        let model_dir = temp_dir.path().join("models");
        fs::write(&input, "pdf").expect("input");
        fs::create_dir(&model_dir).expect("model dir");
        fs::create_dir(model_dir.join("PP-OCRv6_medium_det")).expect("det model dir");
        fs::create_dir(model_dir.join("PP-OCRv6_medium_rec")).expect("rec model dir");
        for model_name in ["PP-OCRv6_medium_det", "PP-OCRv6_medium_rec"] {
            for file_name in ["inference.json", "inference.pdiparams", "inference.yml"] {
                fs::write(model_dir.join(model_name).join(file_name), "model").expect("model file");
            }
        }

        validate_ocr_inputs(&input, &model_dir, "medium").expect("inputs valid");
        let request = build_ocr_request(&input, &model_dir, "medium");

        assert_eq!(request.tier, "medium");
        assert_eq!(request.max_pdf_pages, 12);
        assert!(request.file_path.ends_with("scan.pdf"));
    }

    #[test]
    fn rejects_incomplete_model_files() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let input = temp_dir.path().join("scan.pdf");
        let model_dir = temp_dir.path().join("models");
        fs::write(&input, "pdf").expect("input");
        fs::create_dir(&model_dir).expect("model dir");
        fs::create_dir(model_dir.join("PP-OCRv6_medium_det")).expect("det model dir");
        fs::create_dir(model_dir.join("PP-OCRv6_medium_rec")).expect("rec model dir");

        let error = validate_ocr_inputs(&input, &model_dir, "medium")
            .expect_err("incomplete model files are rejected");

        assert!(error.to_string().contains("inference.json"));
    }

    #[test]
    fn resource_sidecar_path_wins_over_invalid_env_path() {
        let _env_lock = env_lock().lock().expect("env lock");
        let _guard = EnvVarGuard::set("OCR_SIDECAR_PATH", "Z:\\missing\\ocr_sidecar.py");
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let resource_script = temp_dir.path().join("ocr_sidecar.py");
        fs::write(&resource_script, "print('ok')").expect("resource script");

        let resolved =
            resolve_ocr_sidecar_script(Some(&resource_script)).expect("resource path resolves");

        assert_eq!(resolved, resource_script);
    }

    #[test]
    fn resource_environment_checker_path_is_used_first() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let resource_checker = temp_dir.path().join("check_ocr_environment.py");
        fs::write(&resource_checker, "print('{}')").expect("resource checker");

        let resolved =
            resolve_ocr_environment_checker(Some(&resource_checker)).expect("checker resolves");

        assert_eq!(resolved, resource_checker);
    }

    #[test]
    fn parses_successful_sidecar_stdout() {
        let result = parse_ocr_sidecar_stdout(
            r#"{"ok":true,"result":{"text":"OCR 文本","pageCount":1,"pages":[{"pageIndex":0,"text":"OCR 文本","confidence":0.99}]}}"#,
        )
        .expect("sidecar output parses");

        assert_eq!(result.text, "OCR 文本");
        assert_eq!(result.page_count, 1);
        assert_eq!(result.pages[0].page_index, 0);
    }

    #[test]
    fn maps_sidecar_error_to_app_error() {
        let error = parse_ocr_sidecar_stdout(
            r#"{"ok":false,"error":{"code":"OCR_EMPTY_RESULT","message":"没有从文件中识别到文字"}}"#,
        )
        .expect_err("sidecar error maps");

        assert!(error.to_string().contains("OCR_EMPTY_RESULT"));
        assert!(error.to_string().contains("没有从文件中识别到文字"));
    }

    #[test]
    fn parses_failed_environment_report_without_error() {
        let report = parse_ocr_environment_report(
            r#"{"ok":false,"checks":[{"name":"paddleocr","ok":false,"message":"paddleocr missing","details":{"missing":["paddleocr"]}}]}"#,
        )
        .expect("environment report parses");

        assert!(!report.ok);
        assert_eq!(report.checks[0].name, "paddleocr");
        assert!(!report.checks[0].ok);
    }

    #[test]
    fn builds_parsed_document_from_ocr_result() {
        let result = parse_ocr_sidecar_stdout(
            r#"{"ok":true,"result":{"text":"第一页文字\n\n第二页文字","pageCount":2,"pages":[{"pageIndex":0,"text":"第一页文字","confidence":0.9},{"pageIndex":1,"text":"第二页文字","confidence":0.8}]}}"#,
        )
        .expect("sidecar output parses");

        let document = build_ocr_document("扫描资料.pdf", &result).expect("document builds");

        assert_eq!(document.title, "扫描资料.pdf");
        assert_eq!(document.source_locator, "扫描资料.pdf#ocr");
        assert!(document.body.contains("第一页文字"));
        assert!(document.summary.contains("第一页文字"));
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = env::var_os(key);
            env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(previous) = self.previous.take() {
                env::set_var(self.key, previous);
            } else {
                env::remove_var(self.key);
            }
        }
    }
}
