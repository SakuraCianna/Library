use std::env;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::error::AppError;
use crate::models::{VisionSidecarRequest, VisionSidecarResult};

const VISION_SIDECAR_TIMEOUT: Duration = Duration::from_secs(300); // Vision might take a while on CPU

#[derive(Debug, serde::Deserialize)]
struct VisionSidecarEnvelope {
    ok: bool,
    result: Option<VisionSidecarResult>,
    error: Option<VisionSidecarError>,
}

#[derive(Debug, serde::Deserialize)]
struct VisionSidecarError {
    code: String,
    message: String,
}

pub fn run_vision_sidecar_cancellable<F>(
    request: &VisionSidecarRequest,
    resource_script_path: Option<&Path>,
    is_cancelled: F,
) -> Result<VisionSidecarResult, AppError>
where
    F: Fn() -> bool,
{
    let script_path = resolve_vision_sidecar_script(resource_script_path)?;
    let project_root = discover_project_root().ok();
    let python_path = discover_python_executable(project_root.as_deref());

    run_vision_sidecar_with_paths(
        request,
        &python_path,
        &script_path,
        VISION_SIDECAR_TIMEOUT,
        is_cancelled,
    )
}

fn run_vision_sidecar_with_paths<F>(
    request: &VisionSidecarRequest,
    python_path: &Path,
    script_path: &Path,
    timeout: Duration,
    is_cancelled: F,
) -> Result<VisionSidecarResult, AppError>
where
    F: Fn() -> bool,
{
    if !script_path.is_file() {
        return Err(AppError::Filesystem(format!(
            "找不到 Vision sidecar：{}",
            script_path.display()
        )));
    }

    let mut child = Command::new(python_path)
        .arg(script_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| AppError::Filesystem(format!("无法启动 Vision sidecar：{error}")))?;

    let stdout_handle = child
        .stdout
        .take()
        .ok_or_else(|| AppError::Filesystem("无法读取 Vision sidecar stdout".to_string()))
        .map(read_output_pipe)?;
    let stderr_handle = child
        .stderr
        .take()
        .ok_or_else(|| AppError::Filesystem("无法读取 Vision sidecar stderr".to_string()))
        .map(read_output_pipe)?;

    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| AppError::Filesystem("无法写入 Vision sidecar stdin".to_string()))?;
        let payload = serde_json::to_vec(request)
            .map_err(|error| AppError::Filesystem(format!("无法序列化 Vision 请求：{error}")))?;
        stdin
            .write_all(&payload)
            .map_err(|error| AppError::Filesystem(format!("无法发送 Vision 请求：{error}")))?;
    }

    let start = Instant::now();
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| AppError::Filesystem(format!("无法等待 Vision sidecar：{error}")))?
        {
            break status;
        }
        if is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AppError::Filesystem(
                "VISION_CANCELLED：用户取消了 Vision 任务".to_string(),
            ));
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AppError::Filesystem(
                "VISION_TIMEOUT：Vision sidecar 执行超时".to_string(),
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    };

    let stdout = join_output_reader(stdout_handle, "Vision 输出")?;
    let stderr = join_output_reader(stderr_handle, "Vision 日志")?;

    if !status.success() {
        return Err(AppError::Filesystem(format!(
            "Vision sidecar 退出失败：{}",
            truncate_chars(stderr.trim(), 500)
        )));
    }

    parse_vision_sidecar_stdout(&stdout).map_err(|error| {
        AppError::Filesystem(format!(
            "{}；日志：{}",
            error,
            truncate_chars(stderr.trim(), 500)
        ))
    })
}

fn parse_vision_sidecar_stdout(stdout: &str) -> Result<VisionSidecarResult, AppError> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Err(AppError::Filesystem(
            "Vision sidecar 未返回任何输出".to_string(),
        ));
    }

    let last_line = trimmed.lines().last().unwrap_or(trimmed);
    let envelope: VisionSidecarEnvelope = serde_json::from_str(last_line).map_err(|error| {
        AppError::Filesystem(format!("Vision sidecar 返回了无效 JSON：{error}"))
    })?;

    if envelope.ok {
        envelope
            .result
            .ok_or_else(|| AppError::Filesystem("Vision sidecar 报告成功但未提供结果".to_string()))
    } else {
        let message = envelope
            .error
            .map(|e| e.message)
            .unwrap_or_else(|| "未知错误".to_string());
        Err(AppError::Filesystem(format!(
            "Vision sidecar 错误：{message}"
        )))
    }
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

fn resolve_vision_sidecar_script(resource_path: Option<&Path>) -> Result<PathBuf, AppError> {
    if let Some(path) = resource_path {
        let target = path
            .join("sidecars")
            .join("vision")
            .join("vision_sidecar.py");
        if target.is_file() {
            return Ok(target);
        }
    }

    if let Ok(project_root) = discover_project_root() {
        let target = project_root
            .join("sidecars")
            .join("vision")
            .join("vision_sidecar.py");
        if target.is_file() {
            return Ok(target);
        }
    }

    Err(AppError::Filesystem(
        "无法找到 vision_sidecar.py".to_string(),
    ))
}

fn discover_project_root() -> Result<PathBuf, AppError> {
    let current_dir =
        env::current_dir().map_err(|e| AppError::Filesystem(format!("无法获取当前目录：{e}")))?;

    for ancestor in current_dir.ancestors() {
        let cargo_toml = ancestor.join("Cargo.toml");
        if cargo_toml.is_file() {
            if let Some(parent) = ancestor.parent() {
                if parent.join("sidecars").is_dir() {
                    return Ok(parent.to_path_buf());
                }
            }
        }
    }

    Ok(current_dir)
}

fn discover_python_executable(project_root: Option<&Path>) -> PathBuf {
    if let Some(root) = project_root {
        let venv_python = if cfg!(windows) {
            root.join(".venv").join("Scripts").join("python.exe")
        } else {
            root.join(".venv").join("bin").join("python")
        };
        if venv_python.is_file() {
            return venv_python;
        }
    }

    PathBuf::from("python")
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{truncated}...")
    }
}
