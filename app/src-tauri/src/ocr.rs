use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use std::{env, fs};

use crate::error::AppError;
use crate::models::{
    OcrEnvironmentReport, OcrSidecarRequest, OcrSidecarResult, ParsedDocument,
    ParsedDocumentSegment, ParsedEvidenceMetadata,
};

const SUMMARY_CHARS: usize = 180;
const MAX_OCR_BODY_CHARS: usize = 60_000;
const OCR_SIDECAR_TIMEOUT: Duration = Duration::from_secs(120);
const OCR_ENV_CHECK_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_OCR_INPUT_BYTES: u64 = 50 * 1024 * 1024;
const DEFAULT_MAX_OCR_PDF_PAGES: u32 = 12;
const DEFAULT_MAX_OCR_IMAGE_PIXELS: u64 = 25_000_000;
const OCR_TEMP_ROOT_NAME: &str = "library-ocr-runs";
const OCR_TEMP_DIR_PREFIX: &str = "run-";
const REQUIRED_MODEL_FILES: [&str; 3] = ["inference.json", "inference.pdiparams", "inference.yml"];
const DOCX_IMAGE_EXTENSIONS: [&str; 11] = [
    ".bmp", ".emf", ".gif", ".jpeg", ".jpg", ".png", ".svg", ".tif", ".tiff", ".webp", ".wmf",
];
const OCR_SUPPORTED_DOCX_IMAGE_EXTENSIONS: [&str; 7] =
    ["bmp", "jpeg", "jpg", "png", "tif", "tiff", "webp"];

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OcrProgressUpdate {
    pub phase: String,
    pub current: u32,
    pub total: u32,
}

pub struct PreparedOcrRequest {
    request: OcrSidecarRequest,
    source_locator: String,
    _temp_dir: Option<OcrSidecarTempDir>,
}

impl PreparedOcrRequest {
    pub fn request(&self) -> &OcrSidecarRequest {
        &self.request
    }

    pub fn source_locator(&self) -> &str {
        &self.source_locator
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum OcrSidecarStreamEvent {
    Progress {
        phase: String,
        current: u32,
        total: u32,
    },
    Result {
        response: OcrSidecarEnvelope,
    },
}

pub fn build_ocr_request(file_path: &Path, model_dir: &Path, tier: &str) -> OcrSidecarRequest {
    OcrSidecarRequest {
        file_path: file_path.to_string_lossy().to_string(),
        model_dir: model_dir.to_string_lossy().to_string(),
        tier: tier.to_string(),
        max_pdf_pages: ocr_pdf_page_limit(),
        max_image_pixels: ocr_image_pixel_limit(),
        progress: true,
        temp_dir: None,
    }
}

pub fn run_ocr_sidecar_cancellable_with_progress<F, P>(
    request: &OcrSidecarRequest,
    resource_script_path: Option<&Path>,
    is_cancelled: F,
    on_progress: P,
) -> Result<OcrSidecarResult, AppError>
where
    F: Fn() -> bool,
    P: FnMut(OcrProgressUpdate),
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
        on_progress,
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
    on_progress: impl FnMut(OcrProgressUpdate),
) -> Result<OcrSidecarResult, AppError>
where
    F: Fn() -> bool,
{
    if request.progress {
        return run_ocr_sidecar_streaming_with_paths(
            request,
            python_path,
            script_path,
            timeout,
            is_cancelled,
            on_progress,
        );
    }

    if !script_path.is_file() {
        return Err(AppError::Filesystem(format!(
            "找不到 OCR sidecar：{}",
            script_path.display()
        )));
    }

    let temp_dir = OcrSidecarTempDir::create()?;
    let request_payload = request_with_temp_dir(request, temp_dir.path());
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
        let payload = serde_json::to_vec(&request_payload)
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

fn run_ocr_sidecar_streaming_with_paths<F, P>(
    request: &OcrSidecarRequest,
    python_path: &Path,
    script_path: &Path,
    timeout: Duration,
    is_cancelled: F,
    mut on_progress: P,
) -> Result<OcrSidecarResult, AppError>
where
    F: Fn() -> bool,
    P: FnMut(OcrProgressUpdate),
{
    if !script_path.is_file() {
        return Err(AppError::Filesystem(format!(
            "找不到 OCR sidecar：{}",
            script_path.display()
        )));
    }

    let temp_dir = OcrSidecarTempDir::create()?;
    let request_payload = request_with_temp_dir(request, temp_dir.path());
    let mut child = Command::new(python_path)
        .arg(script_path)
        .env("DISABLE_MODEL_SOURCE_CHECK", "True")
        .env("PADDLE_PDX_DISABLE_MODEL_SOURCE_CHECK", "True")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| AppError::Filesystem(format!("无法启动 OCR sidecar：{error}")))?;

    let (stdout_sender, stdout_receiver) = mpsc::channel();
    let stdout_handle = child
        .stdout
        .take()
        .ok_or_else(|| AppError::Filesystem("无法读取 OCR sidecar stdout".to_string()))
        .map(|stdout| forward_output_lines(stdout, stdout_sender))?;
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
        let payload = serde_json::to_vec(&request_payload)
            .map_err(|error| AppError::Filesystem(format!("无法序列化 OCR 请求：{error}")))?;
        stdin
            .write_all(&payload)
            .map_err(|error| AppError::Filesystem(format!("无法发送 OCR 请求：{error}")))?;
    }

    let start = Instant::now();
    let mut final_result: Option<Result<OcrSidecarResult, AppError>> = None;
    let status = loop {
        if let Err(error) =
            drain_ocr_stream_lines(&stdout_receiver, &mut final_result, &mut on_progress)
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
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

    stdout_handle
        .join()
        .map_err(|_| AppError::Filesystem("OCR 输出读取线程异常退出".to_string()))?
        .map_err(|error| AppError::Filesystem(format!("无法读取 OCR 输出：{error}")))?;
    drain_ocr_stream_lines(&stdout_receiver, &mut final_result, &mut on_progress)?;
    let stderr = join_output_reader(stderr_handle, "OCR 日志")?;

    if !status.success() {
        return Err(AppError::Filesystem(format!(
            "OCR sidecar 退出失败：{}",
            truncate_chars(stderr.trim(), 500)
        )));
    }

    final_result.unwrap_or_else(|| {
        Err(AppError::Filesystem(
            "OCR sidecar 未返回最终结果".to_string(),
        ))
    })
}

pub struct OcrSidecarTempDir {
    path: PathBuf,
}

impl OcrSidecarTempDir {
    pub fn create() -> Result<Self, AppError> {
        let root = ocr_temp_root();
        fs::create_dir_all(&root)
            .map_err(|error| AppError::Filesystem(format!("无法创建 OCR 临时目录：{error}")))?;
        let path = root.join(format!("{}{}", OCR_TEMP_DIR_PREFIX, uuid::Uuid::new_v4()));
        fs::create_dir(&path)
            .map_err(|error| AppError::Filesystem(format!("无法创建 OCR 任务临时目录：{error}")))?;

        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for OcrSidecarTempDir {
    fn drop(&mut self) {
        if let Err(error) = remove_controlled_ocr_temp_dir(&self.path) {
            eprintln!(
                "failed to clean OCR temp dir {}: {error}",
                self.path.display()
            );
        }
    }
}

fn request_with_temp_dir(request: &OcrSidecarRequest, temp_dir: &Path) -> OcrSidecarRequest {
    let mut request_payload = request.clone();
    request_payload.temp_dir = Some(temp_dir.to_string_lossy().to_string());
    request_payload
}

fn ocr_temp_root() -> PathBuf {
    env::temp_dir().join(OCR_TEMP_ROOT_NAME)
}

fn remove_controlled_ocr_temp_dir(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let root = ocr_temp_root().canonicalize()?;
    let target = path.canonicalize()?;
    let is_direct_child = target.parent() == Some(root.as_path());
    let has_owned_prefix = target
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with(OCR_TEMP_DIR_PREFIX))
        .unwrap_or(false);

    if !is_direct_child || !has_owned_prefix {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "refusing to remove unowned OCR temp dir",
        ));
    }

    fs::remove_dir_all(target)
}

pub fn docx_embedded_image_target(docx_path: &Path, image_number: u32) -> Result<String, AppError> {
    let mut archive = open_docx_archive(docx_path)?;
    let archive_names = archive
        .file_names()
        .map(|name| name.to_string())
        .collect::<HashSet<_>>();
    let document_xml = read_docx_zip_text(&mut archive, "word/document.xml")?;
    let relationships_xml =
        read_docx_zip_text(&mut archive, "word/_rels/document.xml.rels").unwrap_or_default();
    let image_targets =
        docx_referenced_image_targets(&document_xml, &relationships_xml, &archive_names);
    let target_index = usize::try_from(image_number.saturating_sub(1)).unwrap_or(usize::MAX);

    image_targets.get(target_index).cloned().ok_or_else(|| {
        AppError::Filesystem(format!(
            "DOCX 内未找到可 OCR 的第 {image_number} 张内嵌图片"
        ))
    })
}

pub fn extract_docx_image_to_path(
    docx_path: &Path,
    image_target: &str,
    output_path: &Path,
) -> Result<(), AppError> {
    let mut archive = open_docx_archive(docx_path)?;
    let mut image_file = archive.by_name(image_target).map_err(|error| {
        AppError::Filesystem(format!("无法读取 DOCX 内嵌图片 {image_target}：{error}"))
    })?;
    if image_file.size() > MAX_OCR_INPUT_BYTES {
        return Err(AppError::Filesystem(format!(
            "OCR_INPUT_TOO_LARGE：DOCX 内嵌图片过大，当前上限为 {} MB",
            MAX_OCR_INPUT_BYTES / 1024 / 1024
        )));
    }
    let mut image_bytes = Vec::new();
    image_file
        .read_to_end(&mut image_bytes)
        .map_err(|error| AppError::Filesystem(format!("无法解压 DOCX 内嵌图片：{error}")))?;
    fs::write(output_path, image_bytes)
        .map_err(|error| AppError::Filesystem(format!("无法写入 OCR 临时图片：{error}")))?;
    Ok(())
}

fn open_docx_archive(path: &Path) -> Result<zip::ZipArchive<fs::File>, AppError> {
    let file = fs::File::open(path)
        .map_err(|error| AppError::Filesystem(format!("无法打开 DOCX 文件：{error}")))?;
    zip::ZipArchive::new(file)
        .map_err(|error| AppError::Filesystem(format!("无法读取 DOCX 压缩结构：{error}")))
}

fn read_docx_zip_text(
    archive: &mut zip::ZipArchive<fs::File>,
    name: &str,
) -> Result<String, AppError> {
    let mut file = archive
        .by_name(name)
        .map_err(|error| AppError::Filesystem(format!("无法读取 DOCX 内容 {name}：{error}")))?;
    let mut content = String::new();
    file.read_to_string(&mut content)
        .map_err(|error| AppError::Filesystem(format!("无法解析 DOCX XML {name}：{error}")))?;
    Ok(content)
}

fn docx_referenced_image_targets(
    document_xml: &str,
    relationships_xml: &str,
    archive_names: &HashSet<String>,
) -> Vec<String> {
    let relationships = docx_image_relationship_targets(relationships_xml, archive_names);
    extract_xml_start_tags(document_xml, "blip")
        .into_iter()
        .filter_map(|tag| {
            let relationship_id = xml_attribute(tag, "r:embed")
                .or_else(|| xml_attribute(tag, "embed"))
                .or_else(|| xml_attribute(tag, "r:link"))
                .or_else(|| xml_attribute(tag, "link"))?;
            relationships.get(&relationship_id).cloned()
        })
        .collect()
}

fn docx_image_relationship_targets(
    relationships_xml: &str,
    archive_names: &HashSet<String>,
) -> HashMap<String, String> {
    extract_xml_start_tags(relationships_xml, "Relationship")
        .into_iter()
        .filter_map(|tag| {
            let relationship_id = xml_attribute(tag, "Id")?;
            let relationship_type = xml_attribute(tag, "Type").unwrap_or_default();
            let target = normalize_docx_relationship_target(&xml_attribute(tag, "Target")?)?;
            if relationship_type.trim().ends_with("/image")
                && archive_names.contains(&target)
                && target.starts_with("word/media/")
                && is_docx_image_target(&target)
            {
                Some((relationship_id, target))
            } else {
                None
            }
        })
        .collect()
}

fn normalize_docx_relationship_target(target: &str) -> Option<String> {
    let normalized = unescape_xml(target).trim().replace('\\', "/");
    if normalized.is_empty() || normalized.contains('\0') {
        return None;
    }
    if normalized
        .split('/')
        .next()
        .map(|part| part.contains(':'))
        .unwrap_or(false)
    {
        return None;
    }
    let joined = if normalized.starts_with('/') {
        normalized.trim_start_matches('/').to_string()
    } else if normalized.starts_with("word/") {
        normalized
    } else {
        format!("word/{normalized}")
    };
    let parts = joined.split('/').collect::<Vec<_>>();
    if parts
        .iter()
        .any(|part| part.is_empty() || *part == "." || *part == "..")
    {
        return None;
    }
    Some(parts.join("/"))
}

fn is_docx_image_target(target: &str) -> bool {
    let target = target.to_ascii_lowercase();
    DOCX_IMAGE_EXTENSIONS
        .iter()
        .any(|extension| target.ends_with(extension))
}

pub fn is_ocr_supported_docx_image_extension(extension: &str) -> bool {
    let extension = extension.trim_start_matches('.').to_ascii_lowercase();
    OCR_SUPPORTED_DOCX_IMAGE_EXTENSIONS
        .iter()
        .any(|supported| *supported == extension)
}

fn extract_xml_start_tags<'a>(xml: &'a str, local_name: &str) -> Vec<&'a str> {
    let mut tags = Vec::new();
    let mut cursor = xml;
    while let Some(start) = cursor.find('<') {
        let candidate = &cursor[start..];
        let Some(end) = candidate.find('>') else {
            break;
        };
        let tag = &candidate[..=end];
        if xml_tag_local_name(tag) == Some(local_name) {
            tags.push(tag);
        }
        cursor = &candidate[end + 1..];
    }
    tags
}

fn xml_tag_local_name(tag: &str) -> Option<&str> {
    let trimmed = tag.trim_start_matches('<').trim_start();
    if trimmed.starts_with('/') || trimmed.starts_with('?') || trimmed.starts_with('!') {
        return None;
    }
    let name_end = trimmed
        .find(|character: char| character.is_whitespace() || character == '>' || character == '/')
        .unwrap_or(trimmed.len());
    let name = &trimmed[..name_end];
    name.rsplit(':').next()
}

fn xml_attribute(tag: &str, attribute: &str) -> Option<String> {
    let marker = format!("{attribute}=\"");
    let start = tag.find(&marker)? + marker.len();
    let rest = &tag[start..];
    let end = rest.find('"')?;
    Some(unescape_xml(&rest[..end]))
}

fn unescape_xml(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

pub fn embedded_image_number_from_locator(source_locator: &str) -> Option<u32> {
    source_locator.split('#').skip(1).find_map(|fragment| {
        let number = fragment.strip_prefix("image-")?;
        if number.is_empty() || !number.chars().all(|character| character.is_ascii_digit()) {
            return None;
        }
        number.parse::<u32>().ok()
    })
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
        .arg("--max-pdf-pages")
        .arg(ocr_pdf_page_limit().to_string())
        .arg("--max-image-pixels")
        .arg(ocr_image_pixel_limit().to_string())
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

fn forward_output_lines<R>(
    reader: R,
    sender: mpsc::Sender<String>,
) -> std::thread::JoinHandle<std::io::Result<()>>
where
    R: Read + Send + 'static,
{
    std::thread::spawn(move || {
        let reader = BufReader::new(reader);
        for line in reader.lines() {
            let line = line?;
            if sender.send(line).is_err() {
                break;
            }
        }
        Ok(())
    })
}

fn drain_ocr_stream_lines<P>(
    receiver: &mpsc::Receiver<String>,
    final_result: &mut Option<Result<OcrSidecarResult, AppError>>,
    on_progress: &mut P,
) -> Result<(), AppError>
where
    P: FnMut(OcrProgressUpdate),
{
    while let Ok(line) = receiver.try_recv() {
        handle_ocr_stream_line(&line, final_result, on_progress)?;
    }

    Ok(())
}

fn handle_ocr_stream_line<P>(
    line: &str,
    final_result: &mut Option<Result<OcrSidecarResult, AppError>>,
    on_progress: &mut P,
) -> Result<(), AppError>
where
    P: FnMut(OcrProgressUpdate),
{
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    match serde_json::from_str::<OcrSidecarStreamEvent>(trimmed) {
        Ok(OcrSidecarStreamEvent::Progress {
            phase,
            current,
            total,
        }) => {
            on_progress(OcrProgressUpdate {
                phase,
                current,
                total,
            });
            Ok(())
        }
        Ok(OcrSidecarStreamEvent::Result { response }) => {
            *final_result = Some(ocr_envelope_to_result(response));
            Ok(())
        }
        Err(_) => {
            let envelope: OcrSidecarEnvelope = serde_json::from_str(trimmed).map_err(|error| {
                AppError::Filesystem(format!("OCR sidecar 返回了无效 JSON：{error}"))
            })?;
            *final_result = Some(ocr_envelope_to_result(envelope));
            Ok(())
        }
    }
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

fn ocr_image_pixel_limit() -> u64 {
    env::var("OCR_MAX_IMAGE_PIXELS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_OCR_IMAGE_PIXELS)
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

    ocr_envelope_to_result(envelope)
}

fn ocr_envelope_to_result(envelope: OcrSidecarEnvelope) -> Result<OcrSidecarResult, AppError> {
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

    let file_name = display_file_name(relative_path);
    let segments = result
        .pages
        .iter()
        .filter_map(|page| {
            let page_body = normalize_text(&page.text);
            if page_body.is_empty() {
                return None;
            }
            let page_number = page.page_index.saturating_add(1);
            Some(ParsedDocumentSegment {
                title: format!("{file_name} · OCR 第 {page_number} 页"),
                body: truncate_chars(&page_body, MAX_OCR_BODY_CHARS),
                source_locator: format!("{relative_path}#ocr-page-{page_number:03}"),
                evidence: Some(ParsedEvidenceMetadata {
                    kind: Some("ocr_page".to_string()),
                    page_number: Some(page_number),
                    page_count: Some(result.page_count),
                    image_number: None,
                    line_count: Some(page.line_count.unwrap_or_else(|| line_count(&page.text))),
                    char_count: Some(page.char_count.unwrap_or_else(|| char_count(&page_body))),
                    confidence_percent: page.confidence.map(confidence_percent),
                }),
            })
        })
        .collect();

    Ok(ParsedDocument {
        title: file_name,
        summary: truncate_chars(&body, SUMMARY_CHARS),
        body: truncate_chars(&body, MAX_OCR_BODY_CHARS),
        source_locator: format!("{relative_path}#ocr"),
        segments,
        table_insights: Vec::new(),
    })
}

fn normalize_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn line_count(value: &str) -> u32 {
    let count = value.lines().filter(|line| !line.trim().is_empty()).count();
    u32::try_from(count.max(1)).unwrap_or(u32::MAX)
}

fn char_count(value: &str) -> u32 {
    u32::try_from(value.chars().count()).unwrap_or(u32::MAX)
}

fn confidence_percent(confidence: f32) -> u32 {
    confidence.clamp(0.0, 1.0).mul_add(100.0, 0.0).round() as u32
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
    use std::io::Write;
    use std::path::Path;
    use std::sync::{Mutex, OnceLock};
    use std::{env, fs};

    use super::{
        build_ocr_document, build_ocr_request, handle_ocr_stream_line,
        parse_ocr_environment_report, parse_ocr_sidecar_stdout,
        prepare_docx_embedded_image_ocr_request, request_with_temp_dir,
        resolve_ocr_environment_checker, resolve_ocr_sidecar_script, validate_ocr_inputs,
        OcrProgressUpdate, OcrSidecarTempDir,
    };

    #[test]
    fn validates_existing_file_and_model_dir() {
        let _env_lock = env_lock().lock().expect("env lock");
        let _page_guard = EnvVarGuard::set("OCR_MAX_PDF_PAGES", "12");
        let _image_guard = EnvVarGuard::set("OCR_MAX_IMAGE_PIXELS", "25000000");
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
        assert_eq!(request.max_image_pixels, 25_000_000);
        assert!(request.progress);
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
    fn rejects_ocr_input_over_50_mb_before_model_validation() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let input = temp_dir.path().join("oversized.pdf");
        let model_dir = temp_dir.path().join("models");
        let file = fs::File::create(&input).expect("input file");
        file.set_len(super::MAX_OCR_INPUT_BYTES + 1)
            .expect("sparse oversized input");

        let error = validate_ocr_inputs(&input, &model_dir, "medium")
            .expect_err("oversized OCR input is rejected");

        assert!(error.to_string().contains("OCR_INPUT_TOO_LARGE"));
        assert!(error.to_string().contains("50 MB"));
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
    fn sidecar_temp_dir_is_removed_when_guard_drops() {
        let temp_dir = OcrSidecarTempDir::create().expect("temp dir created");
        let path = temp_dir.path().to_path_buf();
        fs::write(path.join("page-1.pdf"), "sensitive page").expect("temp page written");

        assert!(path.is_dir());
        drop(temp_dir);

        assert!(!path.exists());
    }

    #[test]
    fn sidecar_request_payload_contains_owned_temp_dir() {
        let temp_dir = OcrSidecarTempDir::create().expect("temp dir created");
        let request = build_ocr_request(
            Path::new("E:\\Knowledge\\scan.pdf"),
            Path::new("E:\\CodeHome\\Library\\models\\ocr\\pp-ocrv6"),
            "medium",
        );

        let payload = request_with_temp_dir(&request, temp_dir.path());
        let expected_temp_dir = temp_dir.path().to_string_lossy().to_string();

        assert_eq!(
            payload.temp_dir.as_deref(),
            Some(expected_temp_dir.as_str())
        );
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
    fn parses_streaming_progress_and_result_events() {
        let mut progress_updates = Vec::new();
        let mut final_result = None;

        handle_ocr_stream_line(
            r#"{"type":"progress","phase":"已识别第 1/2 页","current":1,"total":2}"#,
            &mut final_result,
            &mut |progress| progress_updates.push(progress),
        )
        .expect("progress event parses");
        handle_ocr_stream_line(
            r#"{"type":"result","response":{"ok":true,"result":{"text":"OCR 文本","pageCount":1,"pages":[{"pageIndex":0,"text":"OCR 文本","confidence":0.99}]}}}"#,
            &mut final_result,
            &mut |progress| progress_updates.push(progress),
        )
        .expect("result event parses");

        assert_eq!(
            progress_updates,
            vec![OcrProgressUpdate {
                phase: "已识别第 1/2 页".to_string(),
                current: 1,
                total: 2,
            }]
        );
        let result = final_result
            .expect("final result exists")
            .expect("final result succeeds");
        assert_eq!(result.text, "OCR 文本");
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
            r#"{"ok":true,"result":{"text":"第一页文字\n\n第二页文字","pageCount":2,"pages":[{"pageIndex":0,"text":"第一页文字","confidence":0.9,"lineCount":1,"charCount":5},{"pageIndex":1,"text":"第二页文字","confidence":0.8,"lineCount":1,"charCount":5}]}}"#,
        )
        .expect("sidecar output parses");

        let document = build_ocr_document("扫描资料.pdf", &result).expect("document builds");

        assert_eq!(document.title, "扫描资料.pdf");
        assert_eq!(document.source_locator, "扫描资料.pdf#ocr");
        assert_eq!(document.segments.len(), 2);
        assert_eq!(
            document.segments[0].source_locator,
            "扫描资料.pdf#ocr-page-001"
        );
        assert_eq!(
            document.segments[0]
                .evidence
                .as_ref()
                .and_then(|evidence| evidence.confidence_percent),
            Some(90)
        );
        assert_eq!(
            document.segments[0]
                .evidence
                .as_ref()
                .and_then(|evidence| evidence.page_count),
            Some(2)
        );
        assert_eq!(document.segments[1].title, "扫描资料.pdf · OCR 第 2 页");
        assert!(document.body.contains("第一页文字"));
        assert!(document.summary.contains("第一页文字"));
    }

    #[test]
    fn builds_ocr_evidence_metadata_from_legacy_sidecar_page_shape() {
        let result = parse_ocr_sidecar_stdout(
            r#"{"ok":true,"result":{"text":"第一行文字\n第二行文字","pageCount":1,"pages":[{"pageIndex":0,"text":"第一行文字\n第二行文字","confidence":0.91}]}}"#,
        )
        .expect("legacy sidecar output parses");

        let document = build_ocr_document("扫描资料.pdf", &result).expect("document builds");
        let evidence = document.segments[0]
            .evidence
            .as_ref()
            .expect("evidence metadata is generated");

        assert_eq!(evidence.page_number, Some(1));
        assert_eq!(evidence.page_count, Some(1));
        assert_eq!(evidence.line_count, Some(2));
        assert_eq!(evidence.char_count, Some(11));
        assert_eq!(evidence.confidence_percent, Some(91));
    }

    #[test]
    fn extracts_referenced_docx_embedded_image_for_ocr_request() {
        let _env_lock = env_lock().lock().expect("env lock");
        let _page_guard = EnvVarGuard::set("OCR_MAX_PDF_PAGES", "12");
        let _image_guard = EnvVarGuard::set("OCR_MAX_IMAGE_PIXELS", "25000000");
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let docx_path = temp_dir.path().join("report.docx");
        let model_dir = temp_dir.path().join("models");
        create_complete_model_dir(&model_dir);
        write_docx_with_referenced_and_unused_images(&docx_path);

        let prepared = prepare_docx_embedded_image_ocr_request(
            &docx_path,
            "docs\\report.docx#image-001",
            &model_dir,
            "medium",
        )
        .expect("embedded image ocr request is prepared");

        assert_eq!(prepared.source_locator(), "docs\\report.docx#image-001");
        assert!(prepared
            .request()
            .file_path
            .ends_with("embedded-image-001.png"));
        assert_eq!(
            fs::read(prepared.request().file_path.as_str()).expect("extracted image reads"),
            b"referenced"
        );
    }

    #[test]
    fn preserves_docx_embedded_image_numbering_when_unsupported_images_come_first() {
        let _env_lock = env_lock().lock().expect("env lock");
        let _page_guard = EnvVarGuard::set("OCR_MAX_PDF_PAGES", "12");
        let _image_guard = EnvVarGuard::set("OCR_MAX_IMAGE_PIXELS", "25000000");
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let docx_path = temp_dir.path().join("mixed.docx");
        let model_dir = temp_dir.path().join("models");
        create_complete_model_dir(&model_dir);
        write_docx_with_svg_then_png(&docx_path);

        let unsupported = prepare_docx_embedded_image_ocr_request(
            &docx_path,
            "docs\\mixed.docx#image-001",
            &model_dir,
            "medium",
        );
        match unsupported {
            Ok(_) => panic!("svg image should be rejected without renumbering later images"),
            Err(error) => assert!(error.to_string().contains("不支持 OCR")),
        }

        let prepared = prepare_docx_embedded_image_ocr_request(
            &docx_path,
            "docs\\mixed.docx#image-002",
            &model_dir,
            "medium",
        )
        .expect("second embedded image keeps parser numbering");

        assert!(prepared
            .request()
            .file_path
            .ends_with("embedded-image-002.png"));
        assert_eq!(
            fs::read(prepared.request().file_path.as_str()).expect("extracted image reads"),
            b"second-png"
        );
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

    fn create_complete_model_dir(model_dir: &Path) {
        fs::create_dir(model_dir).expect("model dir");
        for model_name in ["PP-OCRv6_medium_det", "PP-OCRv6_medium_rec"] {
            let model_path = model_dir.join(model_name);
            fs::create_dir(&model_path).expect("model subdir");
            for file_name in ["inference.json", "inference.pdiparams", "inference.yml"] {
                fs::write(model_path.join(file_name), "model").expect("model file");
            }
        }
    }

    fn write_docx_with_referenced_and_unused_images(path: &Path) {
        let file = fs::File::create(path).expect("docx file");
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        zip.start_file("word/document.xml", options)
            .expect("document xml starts");
        zip.write_all(
            br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"><w:body><w:p><w:r><w:drawing><wp:inline><wp:docPr id="1" name="Picture 1" descr="Architecture diagram"/><a:graphic><a:graphicData><a:pic><a:blipFill><a:blip r:embed="rId5"/></a:blipFill></a:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p></w:body></w:document>"#,
        )
        .expect("document xml writes");

        zip.start_file("word/_rels/document.xml.rels", options)
            .expect("rels starts");
        zip.write_all(
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/><Relationship Id="rId6" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/unused.png"/></Relationships>"#,
        )
        .expect("rels writes");

        zip.start_file("word/media/image1.png", options)
            .expect("image starts");
        zip.write_all(b"referenced").expect("image writes");
        zip.start_file("word/media/unused.png", options)
            .expect("unused starts");
        zip.write_all(b"unused").expect("unused writes");
        zip.finish().expect("docx finalized");
    }

    fn write_docx_with_svg_then_png(path: &Path) {
        let file = fs::File::create(path).expect("docx file");
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        zip.start_file("word/document.xml", options)
            .expect("document xml starts");
        zip.write_all(
            br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"><w:body><w:p><w:r><w:drawing><wp:inline><a:graphic><a:graphicData><a:pic><a:blipFill><a:blip r:embed="rIdSvg"/></a:blipFill></a:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p><w:p><w:r><w:drawing><wp:inline><a:graphic><a:graphicData><a:pic><a:blipFill><a:blip r:embed="rIdPng"/></a:blipFill></a:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p></w:body></w:document>"#,
        )
        .expect("document xml writes");

        zip.start_file("word/_rels/document.xml.rels", options)
            .expect("rels starts");
        zip.write_all(
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdSvg" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/diagram.svg"/><Relationship Id="rIdPng" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/scan.png"/></Relationships>"#,
        )
        .expect("rels writes");

        zip.start_file("word/media/diagram.svg", options)
            .expect("svg starts");
        zip.write_all(b"<svg></svg>").expect("svg writes");
        zip.start_file("word/media/scan.png", options)
            .expect("png starts");
        zip.write_all(b"second-png").expect("png writes");
        zip.finish().expect("docx finalized");
    }
}

pub fn prepare_file_ocr_request(
    file_path: &Path,
    source_locator: &str,
    model_dir: &Path,
    tier: &str,
) -> Result<PreparedOcrRequest, AppError> {
    validate_ocr_inputs(file_path, model_dir, tier)?;
    Ok(PreparedOcrRequest {
        request: build_ocr_request(file_path, model_dir, tier),
        source_locator: source_locator.to_string(),
        _temp_dir: None,
    })
}

pub fn prepare_docx_embedded_image_ocr_request(
    docx_path: &Path,
    source_locator: &str,
    model_dir: &Path,
    tier: &str,
) -> Result<PreparedOcrRequest, AppError> {
    let image_number = embedded_image_number_from_locator(source_locator).ok_or_else(|| {
        AppError::Filesystem("DOCX 内嵌图片 OCR 缺少有效的 #image-N 来源定位".to_string())
    })?;
    let image_target = docx_embedded_image_target(docx_path, image_number)?;
    let temp_dir = OcrSidecarTempDir::create()?;
    let extension = Path::new(&image_target)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();
    if !is_ocr_supported_docx_image_extension(&extension) {
        return Err(AppError::Filesystem(format!(
            "DOCX 第 {image_number} 张内嵌图片格式 .{extension} 当前不支持 OCR"
        )));
    }
    let image_path = temp_dir
        .path()
        .join(format!("embedded-image-{image_number:03}.{extension}"));
    extract_docx_image_to_path(docx_path, &image_target, &image_path)?;
    validate_ocr_inputs(&image_path, model_dir, tier)?;

    Ok(PreparedOcrRequest {
        request: build_ocr_request(&image_path, model_dir, tier),
        source_locator: source_locator.to_string(),
        _temp_dir: Some(temp_dir),
    })
}
