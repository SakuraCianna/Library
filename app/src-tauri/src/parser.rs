use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::error::AppError;
use crate::models::{FileParseCandidate, ParsedDocument};

const MAX_BODY_CHARS: usize = 60_000;
const SUMMARY_CHARS: usize = 180;

pub fn parse_file(
    root_path: &Path,
    candidate: &FileParseCandidate,
) -> Result<ParsedDocument, AppError> {
    let file_path = resolve_file_path(root_path, &candidate.relative_path)?;
    let extension = candidate.extension.trim_start_matches('.').to_lowercase();
    let raw_text = match extension.as_str() {
        "md" | "txt" => read_text_lossy(&file_path)?,
        "docx" => read_docx_text(&file_path)?,
        "xlsx" => read_xlsx_text(&file_path)?,
        "pdf" => read_pdf_text_lossy(&file_path)?,
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
    })
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

fn read_xlsx_text(path: &Path) -> Result<String, AppError> {
    let mut archive = open_zip(path)?;
    let mut parts = Vec::new();

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
        let text = xml_to_text(&xml);

        if !text.is_empty() {
            parts.push(text);
        }
    }

    Ok(parts.join("\n"))
}

fn open_zip(path: &Path) -> Result<zip::ZipArchive<File>, AppError> {
    let file = File::open(path)
        .map_err(|error| AppError::Filesystem(format!("无法打开压缩文档：{error}")))?;
    zip::ZipArchive::new(file)
        .map_err(|error| AppError::Filesystem(format!("无法读取压缩文档结构：{error}")))
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
    use std::fs;

    use super::parse_file;
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
    }
}
