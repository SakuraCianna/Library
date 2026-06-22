use std::fs::File;
use std::io;
use std::path::Path;

use time::OffsetDateTime;
use walkdir::{DirEntry, WalkDir};

use crate::models::ScannedFile;

const SUPPORTED_EXTENSIONS: [&str; 12] = [
    "pdf", "docx", "xlsx", "md", "txt", "png", "jpg", "jpeg", "bmp", "tif", "tiff", "webp",
];
const SKIPPED_DIR_NAMES: [&str; 7] = [
    ".git",
    ".idea",
    ".vscode",
    "node_modules",
    "target",
    "dist",
    ".venv",
];
const DEFAULT_MAX_SCANNED_FILES: usize = 10_000;
const DEFAULT_MAX_SCANNED_TOTAL_BYTES: u64 = 10 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
struct ScanLimits {
    max_files: usize,
    max_total_bytes: u64,
}

impl Default for ScanLimits {
    fn default() -> Self {
        Self {
            max_files: DEFAULT_MAX_SCANNED_FILES,
            max_total_bytes: DEFAULT_MAX_SCANNED_TOTAL_BYTES,
        }
    }
}

#[cfg(test)]
pub fn scan_folder(root_path: &Path) -> io::Result<Vec<ScannedFile>> {
    scan_folder_with_progress(root_path, |_| true)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanProgress {
    pub scanned_files: u32,
    pub current_path: String,
}

pub fn scan_folder_with_progress<F>(
    root_path: &Path,
    mut on_progress: F,
) -> io::Result<Vec<ScannedFile>>
where
    F: FnMut(&ScanProgress) -> bool,
{
    scan_folder_with_progress_and_limits(root_path, &mut on_progress, ScanLimits::default())
}

fn scan_folder_with_progress_and_limits<F>(
    root_path: &Path,
    mut on_progress: F,
    limits: ScanLimits,
) -> io::Result<Vec<ScannedFile>>
where
    F: FnMut(&ScanProgress) -> bool,
{
    let root_path = root_path.canonicalize()?;
    let mut files = Vec::new();
    let mut total_bytes = 0_u64;

    for entry in WalkDir::new(&root_path)
        .into_iter()
        .filter_entry(|entry| should_visit(entry))
    {
        let entry = entry.map_err(|error| io::Error::new(io::ErrorKind::Other, error))?;
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let Some(extension) = supported_extension(path) else {
            continue;
        };

        let metadata = entry.metadata()?;
        let next_file_count = files.len() + 1;
        if next_file_count > limits.max_files {
            return Err(scan_limit_error(format!(
                "SCAN_TOO_MANY_FILES：扫描文件数量超过 {} 个上限，请拆分目录后再扫描",
                limits.max_files
            )));
        }

        let next_total_bytes = total_bytes.saturating_add(metadata.len());
        if next_total_bytes > limits.max_total_bytes {
            return Err(scan_limit_error(format!(
                "SCAN_TOTAL_BYTES_TOO_LARGE：扫描文件总大小超过 {} 上限，请缩小目录范围后再扫描",
                format_byte_limit(limits.max_total_bytes)
            )));
        }
        total_bytes = next_total_bytes;

        let modified_at = metadata
            .modified()
            .map(OffsetDateTime::from)
            .unwrap_or_else(|_| OffsetDateTime::now_utc())
            .to_string();
        let relative_path = normalized_relative_path(&root_path, path);
        let content_hash = hash_file(path)?;

        files.push(ScannedFile {
            relative_path: relative_path.clone(),
            extension,
            size_bytes: metadata.len() as i64,
            modified_at,
            content_hash,
        });

        let progress = ScanProgress {
            scanned_files: files.len() as u32,
            current_path: relative_path,
        };
        if !on_progress(&progress) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "SCAN_CANCELLED"));
        }
    }

    files.sort_by(|left, right| {
        left.relative_path
            .to_lowercase()
            .cmp(&right.relative_path.to_lowercase())
    });
    Ok(files)
}

fn scan_limit_error(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn format_byte_limit(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{} MB", bytes / 1024 / 1024)
    } else {
        format!("{bytes} 字节")
    }
}

fn should_visit(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return true;
    }

    let name = entry.file_name().to_string_lossy();
    if name.starts_with('.') {
        return false;
    }

    if entry.file_type().is_dir() {
        return !SKIPPED_DIR_NAMES
            .iter()
            .any(|skipped_name| name.eq_ignore_ascii_case(skipped_name));
    }

    true
}

fn supported_extension(path: &Path) -> Option<String> {
    let extension = path.extension()?.to_string_lossy().to_lowercase();
    SUPPORTED_EXTENSIONS
        .contains(&extension.as_str())
        .then_some(extension)
}

fn normalized_relative_path(root_path: &Path, path: &Path) -> String {
    let relative_path = path.strip_prefix(root_path).unwrap_or(path);
    relative_path
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("\\")
}

fn hash_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    io::copy(&mut file, &mut hasher)?;
    Ok(hasher.finalize().to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{scan_folder, scan_folder_with_progress_and_limits, ScanLimits};

    #[test]
    fn scans_supported_files_and_skips_hidden_or_unsupported_entries() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        fs::write(temp_dir.path().join("README.md"), "hello").expect("write md");
        fs::write(temp_dir.path().join("image.png"), "image").expect("write png");
        fs::create_dir(temp_dir.path().join(".git")).expect("hidden dir");
        fs::write(temp_dir.path().join(".git").join("secret.md"), "skip").expect("write hidden");
        fs::create_dir(temp_dir.path().join("资料")).expect("nested dir");
        fs::write(temp_dir.path().join("资料").join("Redis.PDF"), "pdf").expect("write pdf");

        let scanned = scan_folder(temp_dir.path()).expect("scan succeeds");
        let relative_paths = scanned
            .iter()
            .map(|file| file.relative_path.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            relative_paths,
            vec!["image.png", "README.md", "资料\\Redis.PDF"]
        );
        assert_eq!(scanned[0].extension, "png");
        assert_eq!(scanned[1].extension, "md");
        assert_eq!(scanned[2].extension, "pdf");
    }

    #[test]
    fn hash_changes_when_file_content_changes() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let file_path = temp_dir.path().join("note.txt");
        fs::write(&file_path, "first").expect("write first");
        let first_hash = scan_folder(temp_dir.path()).expect("first scan")[0]
            .content_hash
            .clone();

        fs::write(&file_path, "second").expect("write second");
        let second_hash = scan_folder(temp_dir.path()).expect("second scan")[0]
            .content_hash
            .clone();

        assert_ne!(first_hash, second_hash);
    }

    #[test]
    fn rejects_scan_when_supported_file_count_exceeds_limit() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        fs::write(temp_dir.path().join("one.md"), "one").expect("write one");
        fs::write(temp_dir.path().join("two.md"), "two").expect("write two");
        fs::write(temp_dir.path().join("three.md"), "three").expect("write three");

        let error = scan_folder_with_progress_and_limits(
            temp_dir.path(),
            |_| true,
            ScanLimits {
                max_files: 2,
                max_total_bytes: u64::MAX,
            },
        )
        .expect_err("scan limit rejects too many files");
        let message = error.to_string();

        assert!(message.contains("SCAN_TOO_MANY_FILES"));
        assert!(!message.contains(temp_dir.path().to_string_lossy().as_ref()));
    }

    #[test]
    fn rejects_scan_when_supported_total_size_exceeds_limit() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        fs::write(temp_dir.path().join("one.md"), "12345").expect("write one");
        fs::write(temp_dir.path().join("two.md"), "67890").expect("write two");

        let error = scan_folder_with_progress_and_limits(
            temp_dir.path(),
            |_| true,
            ScanLimits {
                max_files: 10,
                max_total_bytes: 7,
            },
        )
        .expect_err("scan limit rejects too many bytes");
        let message = error.to_string();

        assert!(message.contains("SCAN_TOTAL_BYTES_TOO_LARGE"));
        assert!(!message.contains(temp_dir.path().to_string_lossy().as_ref()));
    }
}
