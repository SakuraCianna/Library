use std::fs::File;
use std::io;
use std::path::Path;

use time::OffsetDateTime;
use walkdir::{DirEntry, WalkDir};

use crate::models::ScannedFile;

const SUPPORTED_EXTENSIONS: [&str; 5] = ["pdf", "docx", "xlsx", "md", "txt"];
const SKIPPED_DIR_NAMES: [&str; 7] = [
    ".git",
    ".idea",
    ".vscode",
    "node_modules",
    "target",
    "dist",
    ".venv",
];

pub fn scan_folder(root_path: &Path) -> io::Result<Vec<ScannedFile>> {
    let root_path = root_path.canonicalize()?;
    let mut files = Vec::new();

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
        let modified_at = metadata
            .modified()
            .map(OffsetDateTime::from)
            .unwrap_or_else(|_| OffsetDateTime::now_utc())
            .to_string();
        let relative_path = normalized_relative_path(&root_path, path);
        let content_hash = hash_file(path)?;

        files.push(ScannedFile {
            relative_path,
            extension,
            size_bytes: metadata.len() as i64,
            modified_at,
            content_hash,
        });
    }

    files.sort_by(|left, right| {
        left.relative_path
            .to_lowercase()
            .cmp(&right.relative_path.to_lowercase())
    });
    Ok(files)
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

    use super::scan_folder;

    #[test]
    fn scans_supported_files_and_skips_hidden_or_unsupported_entries() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        fs::write(temp_dir.path().join("README.md"), "hello").expect("write md");
        fs::write(temp_dir.path().join("image.png"), "skip").expect("write png");
        fs::create_dir(temp_dir.path().join(".git")).expect("hidden dir");
        fs::write(temp_dir.path().join(".git").join("secret.md"), "skip").expect("write hidden");
        fs::create_dir(temp_dir.path().join("资料")).expect("nested dir");
        fs::write(temp_dir.path().join("资料").join("Redis.PDF"), "pdf").expect("write pdf");

        let scanned = scan_folder(temp_dir.path()).expect("scan succeeds");
        let relative_paths = scanned
            .iter()
            .map(|file| file.relative_path.as_str())
            .collect::<Vec<_>>();

        assert_eq!(relative_paths, vec!["README.md", "资料\\Redis.PDF"]);
        assert_eq!(scanned[0].extension, "md");
        assert_eq!(scanned[1].extension, "pdf");
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
}
