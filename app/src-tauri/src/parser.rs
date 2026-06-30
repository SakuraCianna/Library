use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use std::{env, io::Write};

use crate::error::AppError;
use crate::models::{
    FileParseCandidate, ParsedDocument, ParsedDocumentSegment, ParsedEvidenceMetadata,
    ParsedTableInsight,
};

const MAX_BODY_CHARS: usize = 60_000;
const SUMMARY_CHARS: usize = 180;
const MAX_TABLE_CELL_CHARS: usize = 80;
const TABLE_SAMPLE_ROWS: usize = 3;
const PARSER_SIDECAR_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_PARSER_INPUT_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Debug, serde::Deserialize)]
struct ParserSidecarEnvelope {
    ok: bool,
    result: Option<ParsedDocument>,
    error: Option<ParserSidecarError>,
}

#[derive(Debug, serde::Deserialize)]
struct ParserSidecarError {
    code: String,
    message: String,
}

pub fn parse_file(
    root_path: &Path,
    candidate: &FileParseCandidate,
) -> Result<ParsedDocument, AppError> {
    let file_path = resolve_file_path(root_path, &candidate.relative_path)?;
    let extension = candidate.extension.trim_start_matches('.').to_lowercase();
    let mut table_insights = Vec::new();
    let mut segments = Vec::new();
    let raw_text = match extension.as_str() {
        "md" | "txt" => read_text_lossy(&file_path)?,
        "docx" => read_docx_text(&file_path)?,
        "xlsx" => {
            let analysis = read_xlsx_analysis(&file_path, &candidate.relative_path)?;
            table_insights = analysis.table_insights;
            analysis.body
        }
        "pdf" => {
            let pdf_text = read_pdf_text_lossy(&file_path)?;
            segments.push(ParsedDocumentSegment {
                title: format!("{} · 第 1 页", display_file_name(&candidate.relative_path)),
                body: pdf_text.clone(),
                source_locator: format!("{}#page-001", candidate.relative_path),
                evidence: Some(ParsedEvidenceMetadata {
                    kind: Some("pdf_page".to_string()),
                    page_number: Some(1),
                    page_count: Some(1),
                    image_number: None,
                    line_count: Some(line_count(&pdf_text)),
                    char_count: Some(char_count(&pdf_text)),
                    confidence_percent: None,
                }),
            });
            pdf_text
        }
        _ => {
            return Err(AppError::Filesystem(format!(
                "暂不支持解析 {} 文件",
                candidate.extension
            )));
        }
    };
    let body = normalize_text(&raw_text);

    if body.is_empty() {
        return Err(AppError::Filesystem(format!(
            "没有从 {} 提取到可索引文本",
            candidate.relative_path
        )));
    }

    Ok(ParsedDocument {
        title: display_file_name(&candidate.relative_path),
        summary: summarize(&body),
        body: truncate_chars(&body, MAX_BODY_CHARS),
        source_locator: candidate.relative_path.clone(),
        segments,
        table_insights,
    })
}

pub fn parse_file_with_sidecar(
    root_path: &Path,
    candidate: &FileParseCandidate,
    resource_script_path: Option<&Path>,
    app_data_dir: &Path,
) -> Result<ParsedDocument, AppError> {
    let file_path = resolve_file_path(root_path, &candidate.relative_path)?;

    let ext = candidate.extension.to_lowercase();
    if ["png", "jpg", "jpeg", "bmp", "tif", "tiff", "webp"].contains(&ext.as_str()) {
        let vision_config = crate::runtime::vision_config(app_data_dir);
        let request = crate::models::VisionSidecarRequest {
            file_path: file_path.to_string_lossy().to_string(),
            model_dir: vision_config.model_dir.to_string_lossy().to_string(),
            prompt: "".to_string(),
        };

        if !crate::runtime::runtime_status(app_data_dir).vision.configured {
            let file_name = crate::state::display_relative_file_name(&candidate.relative_path);
            return Ok(ParsedDocument {
                title: file_name.clone(),
                body: "Vision 模型未配置，无法生成图片描述".to_string(),
                summary: "Vision 模型未配置".to_string(),
                source_locator: candidate.relative_path.clone(),
                segments: vec![],
                table_insights: vec![],
            });
        }

        let result = crate::vision::run_vision_sidecar_cancellable(
            &request,
            resource_script_path,
            || false,
        )?;
        
        let file_name = crate::state::display_relative_file_name(&candidate.relative_path);
        return Ok(ParsedDocument {
            title: file_name,
            body: result.caption.clone(),
            summary: result.caption,
            source_locator: candidate.relative_path.clone(),
            segments: vec![],
            table_insights: vec![],
        });
    }

    let script_path = match resolve_parser_sidecar_script(resource_script_path) {
        Ok(script_path) => script_path,
        Err(error) => {
            if parser_sidecar_path_is_explicit() {
                return Err(error);
            }
            return parse_file(root_path, candidate);
        }
    };
    let project_root = discover_project_root().ok();
    let python_path = discover_python_executable(project_root.as_deref())?;

    let mut document = run_parser_sidecar_with_paths(
        &file_path,
        &candidate.relative_path,
        &python_path,
        &script_path,
        PARSER_SIDECAR_TIMEOUT,
    )?;

    if crate::runtime::runtime_status(app_data_dir).vision.configured {
        let vision_config = crate::runtime::vision_config(app_data_dir);
        let model_dir = vision_config.model_dir.to_string_lossy().to_string();

        for segment in &mut document.segments {
            if let Some(evidence) = &segment.evidence {
                if evidence.kind.as_deref() == Some("embedded_image") {
                    let source_locator = &segment.source_locator;
                        if let Some(image_number) = crate::ocr::embedded_image_number_from_locator(source_locator) {
                            if let Ok(image_target) = crate::ocr::docx_embedded_image_target(&file_path, image_number) {
                                let extension = std::path::Path::new(&image_target)
                                    .extension()
                                    .and_then(|e| e.to_str())
                                    .unwrap_or("png")
                                    .to_ascii_lowercase();

                                if crate::ocr::is_ocr_supported_docx_image_extension(&extension) {
                                    if let Ok(temp_dir) = crate::ocr::OcrSidecarTempDir::create() {
                                        let extension_str = extension.as_str();
                                        let image_path = temp_dir.path().join(format!("embedded-image-{image_number:03}.{extension_str}"));
                                        if crate::ocr::extract_docx_image_to_path(&file_path, &image_target, &image_path).is_ok() {
                                            let request = crate::models::VisionSidecarRequest {
                                                file_path: image_path.to_string_lossy().to_string(),
                                                model_dir: model_dir.clone(),
                                                prompt: "".to_string(),
                                            };
                                            if let Ok(result) = crate::vision::run_vision_sidecar_cancellable(
                                                &request,
                                                resource_script_path,
                                                || false,
                                            ) {
                                                if !result.caption.is_empty() {
                                                    segment.body.push_str("\n\n[图片描述] ");
                                                    segment.body.push_str(&result.caption);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

    Ok(document)
}

#[derive(Debug)]
struct XlsxAnalysis {
    body: String,
    table_insights: Vec<ParsedTableInsight>,
}

fn resolve_file_path(root_path: &Path, relative_path: &str) -> Result<PathBuf, AppError> {
    let root = root_path
        .canonicalize()
        .map_err(|error| AppError::Filesystem(format!("无法读取知识库根目录：{error}")))?;
    let relative = relative_path.replace('\\', std::path::MAIN_SEPARATOR_STR);
    let file_path = root.join(relative);
    let canonical = file_path
        .canonicalize()
        .map_err(|error| AppError::Filesystem(format!("无法读取文件：{error}")))?;

    if !canonical.starts_with(&root) {
        return Err(AppError::PermissionDenied(
            "文件路径超出知识库目录边界".to_string(),
        ));
    }

    Ok(canonical)
}

fn read_text_lossy(path: &Path) -> Result<String, AppError> {
    let bytes = fs::read(path)
        .map_err(|error| AppError::Filesystem(format!("无法读取文本文件：{error}")))?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

fn read_docx_text(path: &Path) -> Result<String, AppError> {
    let mut archive = open_zip(path)?;
    let mut document = String::new();
    archive
        .by_name("word/document.xml")
        .map_err(|error| AppError::Filesystem(format!("无法读取 Word 正文：{error}")))?
        .read_to_string(&mut document)
        .map_err(|error| AppError::Filesystem(format!("无法解析 Word 正文：{error}")))?;

    Ok(xml_to_text(&document))
}

fn read_xlsx_analysis(path: &Path, relative_path: &str) -> Result<XlsxAnalysis, AppError> {
    let mut archive = open_zip(path)?;
    let mut shared_strings_xml = String::new();
    let mut sheet_parts = Vec::new();

    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| AppError::Filesystem(format!("无法读取 Excel 内容：{error}")))?;
        let name = file.name().to_string();
        let is_sheet = name.starts_with("xl/worksheets/") && name.ends_with(".xml");
        let is_shared_strings = name == "xl/sharedStrings.xml";

        if !is_sheet && !is_shared_strings {
            continue;
        }

        let mut xml = String::new();
        file.read_to_string(&mut xml)
            .map_err(|error| AppError::Filesystem(format!("无法解析 Excel XML：{error}")))?;

        if is_shared_strings {
            shared_strings_xml = xml;
        } else {
            sheet_parts.push((name, xml));
        }
    }

    sheet_parts.sort_by_key(|(name, _)| worksheet_sort_key(name));

    let mut parts = Vec::new();
    let mut table_insights = Vec::new();
    let shared_strings = parse_shared_strings(&shared_strings_xml);

    let shared_text = xml_to_text(&shared_strings_xml);
    if !shared_text.is_empty() {
        parts.push(shared_text);
    }

    for (index, (_, xml)) in sheet_parts.iter().enumerate() {
        let text = xml_to_text(xml);
        if !text.is_empty() {
            parts.push(text);
        }

        if let Some(insight) =
            worksheet_table_insight(relative_path, index + 1, xml, &shared_strings)
        {
            parts.push(insight.body.clone());
            table_insights.push(insight);
        }
    }

    Ok(XlsxAnalysis {
        body: parts.join("\n"),
        table_insights,
    })
}

fn worksheet_sort_key(name: &str) -> usize {
    name.rsplit('/')
        .next()
        .and_then(|file_name| file_name.strip_prefix("sheet"))
        .and_then(|value| value.strip_suffix(".xml"))
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(usize::MAX)
}

fn parse_shared_strings(xml: &str) -> Vec<String> {
    extract_xml_blocks(xml, "si")
        .into_iter()
        .map(xml_to_text)
        .collect()
}

#[derive(Debug, Default)]
struct WorksheetRowSummary {
    values: Vec<String>,
    column_count: usize,
}

impl WorksheetRowSummary {
    fn has_content(&self) -> bool {
        !self.values.is_empty()
    }
}

fn worksheet_table_insight(
    relative_path: &str,
    sheet_index: usize,
    xml: &str,
    shared_strings: &[String],
) -> Option<ParsedTableInsight> {
    let mut row_count = 0_usize;
    let mut column_count = 0_usize;
    let mut header_values = Vec::new();
    let mut sample_rows = Vec::new();

    visit_xml_blocks(xml, "row", |row_xml| {
        let row = worksheet_row_summary(row_xml, shared_strings);
        if !row.has_content() {
            return;
        }

        row_count += 1;
        column_count = column_count.max(row.column_count);

        if row_count == 1 {
            header_values = row.values;
        } else if sample_rows.len() < TABLE_SAMPLE_ROWS {
            sample_rows.push(row.values);
        }
    });

    if row_count == 0 {
        return None;
    }

    let header_summary = if header_values.is_empty() {
        "未识别表头".to_string()
    } else {
        join_limited(&header_values, "、", 12)
    };
    let title = format!(
        "{} · 工作表 {sheet_index}",
        display_file_name(relative_path)
    );
    let source_locator = format!("{relative_path}#sheet-{sheet_index:03}");
    let mut lines = vec![
        title.clone(),
        format!("来源：{source_locator}"),
        format!("结构：{row_count} 行，{column_count} 列"),
        format!("表头：{header_summary}"),
    ];

    for (sample_index, row) in sample_rows.iter().enumerate() {
        let sample = row_sample(row);
        if !sample.is_empty() {
            lines.push(format!("样例 {}：{sample}", sample_index + 1));
        }
    }

    if !header_values.is_empty() {
        lines.push(format!("可问答字段：{header_summary}"));
    }

    let body = lines.join("\n");
    Some(ParsedTableInsight {
        title,
        summary: format!(
            "工作表 {sheet_index}：{row_count} 行、{column_count} 列；表头：{header_summary}"
        ),
        body,
        source_locator,
    })
}

fn worksheet_row_summary(row_xml: &str, shared_strings: &[String]) -> WorksheetRowSummary {
    let mut values = Vec::new();
    let mut column_count = 0_usize;
    let mut fallback_column_index = 0_usize;

    for cell_xml in extract_xml_blocks(row_xml, "c") {
        let column_index = cell_column_index(cell_xml).unwrap_or(fallback_column_index);
        fallback_column_index = column_index.saturating_add(1);

        if let Some(value) = worksheet_cell_value(cell_xml, shared_strings) {
            column_count = column_count.max(column_index + 1);
            values.push(value);
        }
    }

    WorksheetRowSummary {
        values,
        column_count,
    }
}

fn worksheet_cell_value(cell_xml: &str, shared_strings: &[String]) -> Option<String> {
    let cell_type = xml_attribute(cell_xml, "t");
    let value = if matches!(cell_type.as_deref(), Some("s")) {
        first_xml_tag_text(cell_xml, "v")
            .and_then(|index| index.parse::<usize>().ok())
            .and_then(|index| shared_strings.get(index).cloned())
    } else if matches!(cell_type.as_deref(), Some("inlineStr")) {
        first_xml_tag_text(cell_xml, "is")
    } else {
        first_xml_tag_text(cell_xml, "v")
    }?;

    let value = normalize_text(&value);
    if value.is_empty() {
        None
    } else {
        Some(truncate_chars(&value, MAX_TABLE_CELL_CHARS))
    }
}

fn xml_attribute(xml: &str, attribute: &str) -> Option<String> {
    let marker = format!("{attribute}=\"");
    let start = xml.find(&marker)? + marker.len();
    let rest = &xml[start..];
    let end = rest.find('"')?;
    Some(unescape_xml(&rest[..end]))
}

fn first_xml_tag_text(xml: &str, tag: &str) -> Option<String> {
    let open_marker = format!("<{tag}");
    let close_marker = format!("</{tag}>");
    let start = xml.find(&open_marker)?;
    let rest = &xml[start..];
    let content_start = rest.find('>')? + 1;
    let content = &rest[content_start..];
    let content_end = content.find(&close_marker)?;
    Some(xml_to_text(&content[..content_end]))
}

fn cell_column_index(cell_xml: &str) -> Option<usize> {
    let reference = xml_attribute(cell_xml, "r")?;
    let letters = reference
        .chars()
        .take_while(|character| character.is_ascii_alphabetic())
        .collect::<String>();
    column_letters_to_index(&letters)
}

fn column_letters_to_index(letters: &str) -> Option<usize> {
    let mut index = 0_usize;
    if letters.is_empty() {
        return None;
    }

    for character in letters.chars() {
        let value = character.to_ascii_uppercase() as u8;
        if !value.is_ascii_uppercase() {
            return None;
        }
        index = index * 26 + usize::from(value - b'A' + 1);
    }

    Some(index - 1)
}

fn visit_xml_blocks<'a, F>(xml: &'a str, tag: &str, mut visitor: F)
where
    F: FnMut(&'a str),
{
    let open_marker = format!("<{tag}");
    let close_marker = format!("</{tag}>");
    let mut cursor = xml;

    while let Some(start) = cursor.find(&open_marker) {
        let candidate = &cursor[start..];
        let Some(end) = candidate.find(&close_marker) else {
            break;
        };
        let block_end = end + close_marker.len();
        visitor(&candidate[..block_end]);
        cursor = &candidate[block_end..];
    }
}

fn extract_xml_blocks<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    let mut blocks = Vec::new();
    visit_xml_blocks(xml, tag, |block| blocks.push(block));

    blocks
}

fn non_empty_values(values: &[String]) -> Vec<String> {
    values
        .iter()
        .filter_map(|value| {
            let value = value.trim();
            if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        })
        .collect()
}

fn row_sample(row: &[String]) -> String {
    join_limited(&non_empty_values(row), " | ", 8)
}

fn join_limited(values: &[String], separator: &str, limit: usize) -> String {
    let mut output = values.iter().take(limit).cloned().collect::<Vec<_>>();
    if values.len() > limit {
        output.push(format!("另有 {} 项", values.len() - limit));
    }
    output.join(separator)
}

fn open_zip(path: &Path) -> Result<zip::ZipArchive<File>, AppError> {
    let file = File::open(path)
        .map_err(|error| AppError::Filesystem(format!("无法打开压缩文档：{error}")))?;
    zip::ZipArchive::new(file)
        .map_err(|error| AppError::Filesystem(format!("无法读取压缩文档结构：{error}")))
}

fn run_parser_sidecar_with_paths(
    file_path: &Path,
    relative_path: &str,
    python_path: &Path,
    script_path: &Path,
    timeout: Duration,
) -> Result<ParsedDocument, AppError> {
    if !script_path.is_file() {
        return Err(AppError::Filesystem(format!(
            "找不到文档解析 sidecar：{}",
            script_path.display()
        )));
    }

    let mut child = Command::new(python_path)
        .arg(script_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| AppError::Filesystem(format!("无法启动文档解析 sidecar：{error}")))?;
    let stdout_handle = child
        .stdout
        .take()
        .ok_or_else(|| AppError::Filesystem("无法读取文档解析 sidecar stdout".to_string()))
        .map(read_output_pipe)?;
    let stderr_handle = child
        .stderr
        .take()
        .ok_or_else(|| AppError::Filesystem("无法读取文档解析 sidecar stderr".to_string()))
        .map(read_output_pipe)?;

    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| AppError::Filesystem("无法写入文档解析 sidecar stdin".to_string()))?;
        let payload = serde_json::json!({
            "filePath": file_path.to_string_lossy(),
            "relativePath": relative_path,
            "maxInputBytes": MAX_PARSER_INPUT_BYTES,
        });
        stdin
            .write_all(payload.to_string().as_bytes())
            .map_err(|error| AppError::Filesystem(format!("无法发送文档解析请求：{error}")))?;
    }

    let start = Instant::now();
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| AppError::Filesystem(format!("无法等待文档解析 sidecar：{error}")))?
        {
            break status;
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AppError::Filesystem(
                "PARSER_TIMEOUT：文档解析 sidecar 执行超时".to_string(),
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    };

    let stdout = join_output_reader(stdout_handle, "文档解析输出")?;
    let stderr = join_output_reader(stderr_handle, "文档解析日志")?;

    if !status.success() {
        return Err(AppError::Filesystem(format!(
            "文档解析 sidecar 退出失败：{}",
            truncate_chars(stderr.trim(), 500)
        )));
    }

    parse_parser_sidecar_stdout(&stdout)
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

fn parse_parser_sidecar_stdout(stdout: &str) -> Result<ParsedDocument, AppError> {
    let envelope: ParserSidecarEnvelope = serde_json::from_str(stdout.trim()).map_err(|error| {
        AppError::Filesystem(format!("文档解析 sidecar 返回了无效 JSON：{error}"))
    })?;

    if envelope.ok {
        return envelope
            .result
            .ok_or_else(|| AppError::Filesystem("文档解析 sidecar 缺少 result".to_string()));
    }

    let error = envelope.error.ok_or_else(|| {
        AppError::Filesystem("文档解析 sidecar 返回失败但缺少错误信息".to_string())
    })?;
    Err(AppError::Filesystem(format!(
        "{}：{}",
        error.code, error.message
    )))
}

pub fn resolve_parser_sidecar_script(
    resource_script_path: Option<&Path>,
) -> Result<PathBuf, AppError> {
    if let Ok(explicit_path) = env::var("PARSER_SIDECAR_PATH") {
        let trimmed = explicit_path.trim();
        if !trimmed.is_empty() {
            let explicit_path = PathBuf::from(trimmed);
            if explicit_path.is_file() {
                return Ok(explicit_path);
            }
            return Err(AppError::Filesystem(format!(
                "PARSER_SIDECAR_PATH 指向的 sidecar 不存在：{}",
                explicit_path.display()
            )));
        }
    }

    if let Some(resource_script_path) = resource_script_path.filter(|path| path.is_file()) {
        return Ok(resource_script_path.to_path_buf());
    }

    let project_root = discover_project_root()?;
    Ok(project_root
        .join("sidecars")
        .join("parser")
        .join("parser_sidecar.py"))
}

fn parser_sidecar_path_is_explicit() -> bool {
    env::var("PARSER_SIDECAR_PATH")
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn discover_project_root() -> Result<PathBuf, AppError> {
    let current_dir = env::current_dir()
        .map_err(|error| AppError::Filesystem(format!("无法读取当前目录：{error}")))?;
    current_dir
        .ancestors()
        .find(|path| {
            path.join("sidecars")
                .join("parser")
                .join("parser_sidecar.py")
                .is_file()
        })
        .map(Path::to_path_buf)
        .ok_or_else(|| AppError::Filesystem("找不到项目根目录下的文档解析 sidecar".to_string()))
}

fn discover_python_executable(project_root: Option<&Path>) -> Result<PathBuf, AppError> {
    if let Ok(explicit_path) = env::var("PARSER_PYTHON_PATH") {
        let trimmed = explicit_path.trim();
        if !trimmed.is_empty() {
            let explicit_path = PathBuf::from(trimmed);
            if explicit_path.is_file() {
                return Ok(explicit_path);
            }
            return Err(AppError::Filesystem(format!(
                "PARSER_PYTHON_PATH 指向的 Python 不存在：{}",
                explicit_path.display()
            )));
        }
    }

    if let Some(project_root) = project_root {
        let local_python = project_root
            .join(".venv")
            .join("Scripts")
            .join("python.exe");
        if local_python.is_file() {
            return Ok(local_python);
        }
    }

    Ok(PathBuf::from("python"))
}

fn read_pdf_text_lossy(path: &Path) -> Result<String, AppError> {
    let bytes = fs::read(path)
        .map_err(|error| AppError::Filesystem(format!("无法读取 PDF 文件：{error}")))?;
    let content = String::from_utf8_lossy(&bytes);
    let literal_strings = extract_pdf_literal_strings(&content);

    if literal_strings.chars().count() >= 8 {
        return Ok(literal_strings);
    }

    Ok(extract_readable_runs(&content))
}

fn extract_pdf_literal_strings(content: &str) -> String {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut escaped = false;

    for character in content.chars() {
        if in_string {
            if escaped {
                current.push(character);
                escaped = false;
                continue;
            }

            match character {
                '\\' => escaped = true,
                ')' => {
                    if current.trim().chars().count() >= 2 {
                        values.push(current.trim().to_string());
                    }
                    current.clear();
                    in_string = false;
                }
                _ => current.push(character),
            }
        } else if character == '(' {
            in_string = true;
        }
    }

    values.join("\n")
}

fn extract_readable_runs(content: &str) -> String {
    let mut runs = Vec::new();
    let mut current = String::new();

    for character in content.chars() {
        if is_readable_character(character) {
            current.push(character);
        } else {
            push_readable_run(&mut runs, &mut current);
        }
    }

    push_readable_run(&mut runs, &mut current);
    runs.join("\n")
}

fn push_readable_run(runs: &mut Vec<String>, current: &mut String) {
    let value = normalize_text(current);
    if value.chars().count() >= 4 {
        runs.push(value);
    }
    current.clear();
}

fn is_readable_character(character: char) -> bool {
    character.is_alphanumeric()
        || character.is_whitespace()
        || matches!(
            character,
            '\u{4e00}'
                ..='\u{9fff}'
                    | '，'
                    | '。'
                    | '、'
                    | '；'
                    | '：'
                    | '？'
                    | '！'
                    | ','
                    | '.'
                    | ';'
                    | ':'
                    | '?'
                    | '!'
                    | '-'
                    | '_'
                    | '/'
                    | '\\'
        )
}

fn xml_to_text(xml: &str) -> String {
    let mut output = String::with_capacity(xml.len());
    let mut in_tag = false;

    for character in xml.chars() {
        match character {
            '<' => {
                output.push(' ');
                in_tag = true;
            }
            '>' => {
                output.push(' ');
                in_tag = false;
            }
            _ if !in_tag => output.push(character),
            _ => {}
        }
    }

    normalize_text(&unescape_xml(&output))
}

fn unescape_xml(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

fn normalize_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn summarize(body: &str) -> String {
    truncate_chars(body, SUMMARY_CHARS)
}

fn line_count(value: &str) -> u32 {
    let count = value.lines().filter(|line| !line.trim().is_empty()).count();
    u32::try_from(count.max(1)).unwrap_or(u32::MAX)
}

fn char_count(value: &str) -> u32 {
    u32::try_from(value.chars().count()).unwrap_or(u32::MAX)
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
    use std::env;
    use std::fs::{self, File};
    use std::io::Write;
    use std::path::Path;
    use std::sync::{Mutex, OnceLock};

    use super::{
        discover_python_executable, parse_file, parse_file_with_sidecar,
        parse_parser_sidecar_stdout, resolve_parser_sidecar_script, run_parser_sidecar_with_paths,
    };
    use crate::models::FileParseCandidate;

    #[test]
    fn parses_markdown_as_structured_document() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        fs::write(
            temp_dir.path().join("Redis面试.md"),
            "# Redis\n\n缓存穿透需要空值缓存和布隆过滤器。",
        )
        .expect("write md");
        let document = parse_file(
            temp_dir.path(),
            &FileParseCandidate {
                file_id: "file-redis".to_string(),
                relative_path: "Redis面试.md".to_string(),
                extension: "md".to_string(),
            },
        )
        .expect("markdown parses");

        assert_eq!(document.title, "Redis面试.md");
        assert!(document.body.contains("缓存穿透"));
        assert!(document.summary.contains("Redis"));
    }

    #[test]
    fn extracts_literal_text_from_lightweight_pdf() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        fs::write(
            temp_dir.path().join("note.pdf"),
            "%PDF-1.4\nBT (缓存穿透 需要参数校验) Tj ET\n%%EOF",
        )
        .expect("write pdf");
        let document = parse_file(
            temp_dir.path(),
            &FileParseCandidate {
                file_id: "file-pdf".to_string(),
                relative_path: "note.pdf".to_string(),
                extension: "pdf".to_string(),
            },
        )
        .expect("pdf parses");

        assert!(document.body.contains("缓存穿透"));
        assert_eq!(document.segments.len(), 1);
        assert_eq!(document.segments[0].source_locator, "note.pdf#page-001");
        assert_eq!(
            document.segments[0]
                .evidence
                .as_ref()
                .and_then(|evidence| evidence.page_number),
            Some(1)
        );
    }

    #[test]
    fn extracts_xlsx_worksheet_table_insight() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let workbook_path = temp_dir.path().join("经营报表.xlsx");
        write_test_xlsx(&workbook_path);

        let document = parse_file(
            temp_dir.path(),
            &FileParseCandidate {
                file_id: "file-xlsx".to_string(),
                relative_path: "经营报表.xlsx".to_string(),
                extension: "xlsx".to_string(),
            },
        )
        .expect("xlsx parses");

        assert_eq!(document.title, "经营报表.xlsx");
        assert_eq!(document.table_insights.len(), 1);
        assert!(document.body.contains("经营报表.xlsx · 工作表 1"));
        assert!(document.body.contains("月份、营收、成本"));
        assert!(document.body.contains("2026-06 | 120 | 70"));

        let insight = &document.table_insights[0];
        assert_eq!(insight.source_locator, "经营报表.xlsx#sheet-001");
        assert!(insight.summary.contains("3 行、3 列"));
        assert!(insight.body.contains("可问答字段：月份、营收、成本"));
    }

    #[test]
    fn parses_successful_parser_sidecar_stdout() {
        let document = parse_parser_sidecar_stdout(
            r#"{"ok":true,"result":{"title":"Redis.md","body":"缓存穿透","summary":"缓存穿透","sourceLocator":"Redis.md","segments":[{"title":"Redis.md · 第 1 页","body":"缓存穿透","sourceLocator":"Redis.md#page-001"}],"tableInsights":[]}}"#,
        )
        .expect("parser sidecar stdout parses");

        assert_eq!(document.title, "Redis.md");
        assert_eq!(document.source_locator, "Redis.md");
        assert_eq!(document.segments[0].source_locator, "Redis.md#page-001");
        assert!(document.body.contains("缓存穿透"));
    }

    #[test]
    fn parses_parser_sidecar_stdout_with_embedded_image_evidence() {
        let document = parse_parser_sidecar_stdout(
            r#"{"ok":true,"result":{"title":"架构说明.docx","body":"文档图片占位","summary":"文档图片占位","sourceLocator":"架构说明.docx","segments":[{"title":"架构说明.docx · 文档图片 1","body":"当前仅登记文档内图片","sourceLocator":"架构说明.docx#image-001","evidence":{"kind":"embedded_image","imageNumber":1,"lineCount":4,"charCount":40}}],"tableInsights":[]}}"#,
        )
        .expect("parser sidecar stdout parses");

        let evidence = document.segments[0]
            .evidence
            .as_ref()
            .expect("image evidence is present");
        assert_eq!(
            document.segments[0].source_locator,
            "架构说明.docx#image-001"
        );
        assert_eq!(evidence.kind.as_deref(), Some("embedded_image"));
        assert_eq!(evidence.image_number, Some(1));
    }

    #[test]
    fn parses_legacy_parser_sidecar_stdout_without_segments() {
        let document = parse_parser_sidecar_stdout(
            r#"{"ok":true,"result":{"title":"Redis.md","body":"缓存穿透","summary":"缓存穿透","sourceLocator":"Redis.md","tableInsights":[]}}"#,
        )
        .expect("legacy parser sidecar stdout parses");

        assert_eq!(document.title, "Redis.md");
        assert!(document.segments.is_empty());
    }

    #[test]
    fn rejects_malformed_parser_sidecar_stdout() {
        let error = parse_parser_sidecar_stdout("not-json")
            .expect_err("malformed parser sidecar output is rejected");

        assert!(error
            .to_string()
            .contains("文档解析 sidecar 返回了无效 JSON"));
    }

    #[test]
    fn parser_sidecar_timeout_is_reported() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let input = temp_dir.path().join("Redis.md");
        let script = temp_dir.path().join("sleep_parser.py");
        fs::write(&input, "# Redis").expect("input");
        fs::write(
            &script,
            "import time\nimport sys\nsys.stdin.read()\ntime.sleep(2)\n",
        )
        .expect("script");

        let error = run_parser_sidecar_with_paths(
            &input,
            "Redis.md",
            Path::new("python"),
            &script,
            std::time::Duration::from_millis(50),
        )
        .expect_err("timeout is reported");

        assert!(error.to_string().contains("PARSER_TIMEOUT"));
    }

    #[test]
    fn explicit_parser_sidecar_path_wins_over_resource_path() {
        let _guard = parser_env_lock().lock().expect("parser env lock");
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let explicit_script = temp_dir.path().join("explicit_parser.py");
        let resource_script = temp_dir.path().join("resource_parser.py");
        fs::write(&explicit_script, "# explicit").expect("explicit script");
        fs::write(&resource_script, "# resource").expect("resource script");
        let _env = ScopedEnvVar::set("PARSER_SIDECAR_PATH", &explicit_script);

        let resolved = resolve_parser_sidecar_script(Some(&resource_script))
            .expect("explicit parser sidecar path resolves");

        assert_eq!(resolved, explicit_script);
    }

    #[test]
    fn invalid_explicit_parser_python_path_is_reported() {
        let _guard = parser_env_lock().lock().expect("parser env lock");
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let missing_python = temp_dir.path().join("missing-python.exe");
        let _env = ScopedEnvVar::set("PARSER_PYTHON_PATH", &missing_python);

        let error = discover_python_executable(None)
            .expect_err("invalid explicit parser Python path is rejected");

        assert!(error.to_string().contains("PARSER_PYTHON_PATH"));
    }

    #[test]
    fn parse_file_with_sidecar_handles_missing_resource_path() {
        let _guard = parser_env_lock().lock().expect("parser env lock");
        let _sidecar_env = ScopedEnvVar::remove("PARSER_SIDECAR_PATH");
        let _python_env = ScopedEnvVar::remove("PARSER_PYTHON_PATH");
        let temp_dir = tempfile::tempdir().expect("temp dir");
        fs::write(
            temp_dir.path().join("Redis.md"),
            "# Redis\n\n缓存穿透需要空值缓存。",
        )
        .expect("write md");

        let document = parse_file_with_sidecar(
            temp_dir.path(),
            &FileParseCandidate {
                file_id: "file-redis".to_string(),
                relative_path: "Redis.md".to_string(),
                extension: "md".to_string(),
            },
            Some(&temp_dir.path().join("missing_parser.py")),
            temp_dir.path(),
        )
        .expect("fallback parser succeeds");

        assert!(document.body.contains("缓存穿透"));
    }

    struct ScopedEnvVar {
        name: &'static str,
        previous: Option<String>,
    }

    impl ScopedEnvVar {
        fn set(name: &'static str, value: &Path) -> Self {
            let previous = env::var(name).ok();
            env::set_var(name, value);
            Self { name, previous }
        }

        fn remove(name: &'static str) -> Self {
            let previous = env::var(name).ok();
            env::remove_var(name);
            Self { name, previous }
        }
    }

    impl Drop for ScopedEnvVar {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                env::set_var(self.name, previous);
            } else {
                env::remove_var(self.name);
            }
        }
    }

    fn parser_env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn write_test_xlsx(path: &Path) {
        let file = File::create(path).expect("xlsx file created");
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        zip.start_file("xl/sharedStrings.xml", options)
            .expect("shared strings entry starts");
        zip.write_all(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <si><t></t></si>
  <si><t>月份</t></si>
  <si><t>营收</t></si>
  <si><t>成本</t></si>
  <si><t>2026-06</t></si>
  <si><t>2026-07</t></si>
</sst>"#
                .as_bytes(),
        )
        .expect("shared strings written");

        zip.start_file("xl/worksheets/sheet1.xml", options)
            .expect("worksheet entry starts");
        zip.write_all(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData>
    <row r="1">
      <c r="A1" t="s"><v>1</v></c>
      <c r="B1" t="s"><v>2</v></c>
      <c r="C1" t="s"><v>3</v></c>
    </row>
    <row r="2">
      <c r="A2" t="s"><v>4</v></c>
      <c r="B2"><v>120</v></c>
      <c r="C2"><v>70</v></c>
    </row>
    <row r="3">
      <c r="A3" t="s"><v>5</v></c>
      <c r="B3"><v>140</v></c>
      <c r="C3"><v>80</v></c>
    </row>
  </sheetData>
</worksheet>"#
                .as_bytes(),
        )
        .expect("worksheet written");
        zip.finish().expect("xlsx zip finalized");
    }
}
