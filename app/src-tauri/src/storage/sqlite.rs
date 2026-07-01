use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::models::{
    BackupExport, BackupExportFile, BackupExportKnowledgeBlock, BackupExportMarkdownNote,
    BackupExportParseJob, BackupExportSpace, BackupExportTrashEntry, BackupExportWorkspace,
    FileParseCandidate, KnowledgeBlockContext, KnowledgeBlockSearchHit, KnowledgeFile,
    KnowledgeSpace, ParseJobCandidate, ParseJobSummary, ParseStatus, ParsedDocument,
    ParsedDocumentSegment, ParsedEvidenceMetadata, PermissionMode, ScanSummary, ScannedFile,
    TableInsightPreview,
};

const DOCUMENT_PARSE_EXTENSIONS: [&str; 5] = ["pdf", "docx", "xlsx", "md", "txt"];
const AUTO_OCR_EXTENSIONS: [&str; 7] = ["png", "jpg", "jpeg", "bmp", "tif", "tiff", "webp"];
const KNOWLEDGE_BLOCK_MAX_CHARS: usize = 1_800;
const KNOWLEDGE_BLOCK_MIN_SPLIT_CHARS: usize = 900;
const MAX_DOCUMENT_SEGMENTS: usize = 120;
const MAX_DOCUMENT_SEGMENT_CHARS: usize = 60_000;
const FOUNDATION_SCHEMA: &str = include_str!("../../migrations/001_foundation.sql");
const FOUNDATION_TABLES: [&str; 6] = [
    "knowledge_spaces",
    "files",
    "markdown_notes",
    "knowledge_blocks",
    "parse_jobs",
    "trash_entries",
];

pub struct SpaceRoot {
    pub id: String,
    pub root_path: String,
}

pub struct SqliteStore {
    connection: Connection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DocumentKnowledgeBlock {
    title: String,
    body: String,
    source_locator: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseJobWriteOutcome {
    Updated,
    Cancelled,
    NotRunning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnqueueParseJobResult {
    pub id: String,
    pub inserted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanJobWriteOutcome {
    Updated {
        summary: ScanSummary,
        queued_document_count: u32,
        queued_ocr_count: u32,
    },
    Cancelled,
    NotRunning,
}

impl SqliteStore {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let connection = Connection::open(path)?;
        connection.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;
             PRAGMA foreign_keys = ON;",
        )?;
        let mut store = Self { connection };
        store.apply_foundation_schema()?;
        Ok(store)
    }

    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let connection = Connection::open_in_memory()?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;",
        )?;
        let mut store = Self { connection };
        store.apply_foundation_schema()?;
        Ok(store)
    }

    fn apply_foundation_schema(&mut self) -> rusqlite::Result<()> {
        if self.foundation_schema_needs_rebuild()? {
            self.rebuild_legacy_foundation_schema()?;
        }

        self.connection.execute_batch(FOUNDATION_SCHEMA)?;
        self.ensure_folder_scan_schema()
    }

    fn ensure_folder_scan_schema(&self) -> rusqlite::Result<()> {
        if !self.column_exists("files", "size_bytes")? {
            self.connection.execute_batch(
                "ALTER TABLE files ADD COLUMN size_bytes INTEGER NOT NULL DEFAULT 0;",
            )?;
        }

        if !self.column_exists("files", "last_scanned_at")? {
            self.connection
                .execute_batch("ALTER TABLE files ADD COLUMN last_scanned_at TEXT;")?;
        }

        for (column_name, column_sql) in [
            (
                "source_locator",
                "ALTER TABLE parse_jobs ADD COLUMN source_locator TEXT;",
            ),
            (
                "started_at",
                "ALTER TABLE parse_jobs ADD COLUMN started_at TEXT;",
            ),
            (
                "finished_at",
                "ALTER TABLE parse_jobs ADD COLUMN finished_at TEXT;",
            ),
            (
                "progress_current",
                "ALTER TABLE parse_jobs ADD COLUMN progress_current INTEGER NOT NULL DEFAULT 0;",
            ),
            (
                "progress_total",
                "ALTER TABLE parse_jobs ADD COLUMN progress_total INTEGER NOT NULL DEFAULT 0;",
            ),
            (
                "phase",
                "ALTER TABLE parse_jobs ADD COLUMN phase TEXT NOT NULL DEFAULT '等待执行';",
            ),
        ] {
            if !self.column_exists("parse_jobs", column_name)? {
                self.connection.execute_batch(column_sql)?;
            }
        }

        self.connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS user_settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );"
        )?;

        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> rusqlite::Result<Option<String>> {
        let mut statement = self.connection.prepare("SELECT value FROM user_settings WHERE key = ?")?;
        let mut rows = statement.query([key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn set_setting(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        self.connection.execute(
            "INSERT INTO user_settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn create_knowledge_space(
        &self,
        name: &str,
        root_path: &str,
        default_permission: PermissionMode,
    ) -> rusqlite::Result<String> {
        let id = Uuid::new_v4().to_string();
        let now = OffsetDateTime::now_utc().to_string();
        self.connection.execute(
            "INSERT INTO knowledge_spaces (id, name, root_path, default_permission, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![id, name, root_path, default_permission.as_str(), now],
        )?;
        Ok(id)
    }

    pub fn list_knowledge_spaces(&self) -> rusqlite::Result<Vec<KnowledgeSpace>> {
        let mut statement = self.connection.prepare(
            "SELECT
                space.id,
                space.name,
                space.root_path,
                space.default_permission,
                COALESCE(changed.changed_count, 0) AS changed_count,
                COALESCE(scan_queue.queued_count, 0) AS scan_queue_count,
                COALESCE(document_queue.queued_count, 0) AS document_queue_count,
                COALESCE(ocr_queue.queued_count, 0) AS ocr_queue_count
             FROM knowledge_spaces space
             LEFT JOIN (
                SELECT space_id, COUNT(*) AS changed_count
                FROM files
                WHERE deleted_at IS NULL AND parse_status = 'changed'
                GROUP BY space_id
             ) changed ON changed.space_id = space.id
             LEFT JOIN (
                SELECT space_id, COUNT(*) AS queued_count
                FROM parse_jobs
                WHERE job_type = 'scan' AND status IN ('queued', 'running')
                GROUP BY space_id
             ) scan_queue ON scan_queue.space_id = space.id
             LEFT JOIN (
                SELECT space_id, COUNT(*) AS queued_count
                FROM parse_jobs
                WHERE job_type = 'document' AND status IN ('queued', 'running')
                GROUP BY space_id
             ) document_queue ON document_queue.space_id = space.id
             LEFT JOIN (
                SELECT space_id, COUNT(*) AS queued_count
                FROM parse_jobs
                WHERE job_type = 'ocr' AND status IN ('queued', 'running')
                GROUP BY space_id
             ) ocr_queue ON ocr_queue.space_id = space.id
             WHERE space.deleted_at IS NULL
             ORDER BY space.updated_at DESC, space.name COLLATE NOCASE",
        )?;

        let spaces = statement
            .query_map([], |row| {
                let permission: String = row.get(3)?;
                Ok(KnowledgeSpace {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    default_permission: PermissionMode::from_db(&permission)
                        .unwrap_or(PermissionMode::Readonly),
                    changed_file_count: row.get(4)?,
                    scan_queue_count: row.get(5)?,
                    document_queue_count: row.get(6)?,
                    ocr_queue_count: row.get(7)?,
                })
            })?
            .collect();

        spaces
    }

    pub fn get_space_root(&self, space_id: &str) -> rusqlite::Result<Option<SpaceRoot>> {
        self.connection
            .query_row(
                "SELECT id, root_path FROM knowledge_spaces WHERE id = ?1 AND deleted_at IS NULL",
                [space_id],
                |row| {
                    Ok(SpaceRoot {
                        id: row.get(0)?,
                        root_path: row.get(1)?,
                    })
                },
            )
            .optional()
    }

    pub fn export_space_backup(&self, space_id: &str) -> rusqlite::Result<BackupExport> {
        let space = self.connection.query_row(
            "SELECT id, name, root_path, default_permission, created_at, updated_at
             FROM knowledge_spaces
             WHERE id = ?1 AND deleted_at IS NULL",
            [space_id],
            |row| {
                let permission: String = row.get(3)?;
                Ok(BackupExportSpace {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    root_path: row.get(2)?,
                    default_permission: PermissionMode::from_db(&permission)
                        .unwrap_or(PermissionMode::Readonly),
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            },
        )?;
        let files = self.export_backup_files(space_id)?;
        let file_ids = files
            .iter()
            .map(|file| file.id.clone())
            .collect::<HashSet<_>>();
        let markdown_notes = self.export_backup_markdown_notes(space_id, &file_ids)?;
        let note_ids = markdown_notes
            .iter()
            .map(|note| note.id.clone())
            .collect::<HashSet<_>>();
        let knowledge_blocks =
            self.export_backup_knowledge_blocks(space_id, &file_ids, &note_ids)?;
        let knowledge_block_ids = knowledge_blocks
            .iter()
            .map(|block| block.id.clone())
            .collect::<HashSet<_>>();
        let parse_jobs = self.export_backup_parse_jobs(space_id)?;
        let trash_entries =
            self.export_backup_trash_entries(space_id, &file_ids, &note_ids, &knowledge_block_ids)?;

        Ok(BackupExport {
            format: "library.backup.v1".to_string(),
            schema_version: 1,
            exported_at: OffsetDateTime::now_utc().to_string(),
            workspace: BackupExportWorkspace {
                active_space_id: space.id.clone(),
                default_permission: space.default_permission.clone(),
            },
            space,
            files,
            markdown_notes,
            knowledge_blocks,
            parse_jobs,
            trash_entries,
        })
    }

    pub fn has_knowledge_space(&self, space_id: &str) -> rusqlite::Result<bool> {
        self.connection.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM knowledge_spaces WHERE id = ?1 AND deleted_at IS NULL
             )",
            [space_id],
            |row| row.get::<_, bool>(0),
        )
    }

    pub fn active_space_id_for_root_path(
        &self,
        root_path: &str,
    ) -> rusqlite::Result<Option<String>> {
        self.connection
            .query_row(
                "SELECT id
                 FROM knowledge_spaces
                 WHERE root_path = ?1 COLLATE NOCASE
                   AND deleted_at IS NULL
                 LIMIT 1",
                [root_path],
                |row| row.get(0),
            )
            .optional()
    }

    pub fn restore_space_backup(&mut self, backup: &BackupExport) -> rusqlite::Result<()> {
        let tx = self.connection.transaction()?;
        let space_id = &backup.space.id;

        tx.execute("DELETE FROM trash_entries WHERE space_id = ?1", [space_id])?;
        tx.execute("DELETE FROM parse_jobs WHERE space_id = ?1", [space_id])?;
        tx.execute(
            "DELETE FROM knowledge_blocks WHERE space_id = ?1",
            [space_id],
        )?;
        tx.execute("DELETE FROM markdown_notes WHERE space_id = ?1", [space_id])?;
        tx.execute("DELETE FROM files WHERE space_id = ?1", [space_id])?;
        tx.execute("DELETE FROM knowledge_spaces WHERE id = ?1", [space_id])?;

        tx.execute(
            "INSERT INTO knowledge_spaces (
                id, name, root_path, default_permission, created_at, updated_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                backup.space.id,
                backup.space.name,
                backup.space.root_path,
                backup.space.default_permission.as_str(),
                backup.space.created_at,
                backup.space.updated_at
            ],
        )?;

        for file in &backup.files {
            tx.execute(
                "INSERT INTO files (
                    id, space_id, relative_path, extension, content_hash, size_bytes,
                    modified_at, parse_status, last_scanned_at, created_at, updated_at,
                    deleted_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    file.id,
                    space_id,
                    file.relative_path,
                    file.extension,
                    file.content_hash,
                    file.size_bytes,
                    file.modified_at,
                    file.parse_status,
                    file.last_scanned_at,
                    file.created_at,
                    file.updated_at,
                    file.deleted_at
                ],
            )?;
        }

        for note in &backup.markdown_notes {
            let user_editable = if note.user_editable { 1_i64 } else { 0_i64 };
            tx.execute(
                "INSERT INTO markdown_notes (
                    id, file_id, space_id, relative_path, user_editable,
                    last_generated_hash, created_at, updated_at, deleted_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    note.id,
                    note.file_id,
                    space_id,
                    note.relative_path,
                    user_editable,
                    note.last_generated_hash,
                    note.created_at,
                    note.updated_at,
                    note.deleted_at
                ],
            )?;
        }

        for block in &backup.knowledge_blocks {
            let searchable = if block.searchable { 1_i64 } else { 0_i64 };
            tx.execute(
                "INSERT INTO knowledge_blocks (
                    id, space_id, file_id, note_id, title, body, source_kind,
                    source_locator, searchable, created_at, updated_at, deleted_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    block.id,
                    space_id,
                    block.file_id,
                    block.note_id,
                    block.title,
                    block.body,
                    block.source_kind,
                    block.source_locator,
                    searchable,
                    block.created_at,
                    block.updated_at,
                    block.deleted_at
                ],
            )?;
        }

        for job in &backup.parse_jobs {
            tx.execute(
                "INSERT INTO parse_jobs (
                    id, space_id, file_id, source_locator, job_type, status, error_message,
                    started_at, finished_at, progress_current, progress_total, phase,
                    created_at, updated_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    job.id,
                    space_id,
                    job.file_id,
                    job.source_locator,
                    job.job_type,
                    job.status,
                    job.error_message,
                    job.started_at,
                    job.finished_at,
                    job.progress_current,
                    job.progress_total,
                    job.phase,
                    job.created_at,
                    job.updated_at
                ],
            )?;
        }

        for entry in &backup.trash_entries {
            tx.execute(
                "INSERT INTO trash_entries (
                    id, space_id, entity_kind, entity_id, display_name, original_locator,
                    deleted_at, restored_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    entry.id,
                    space_id,
                    entry.entity_kind,
                    entry.entity_id,
                    entry.display_name,
                    entry.original_locator,
                    entry.deleted_at,
                    entry.restored_at
                ],
            )?;
        }

        tx.commit()
    }

    pub fn update_knowledge_space_permission(
        &self,
        space_id: &str,
        permission: PermissionMode,
    ) -> rusqlite::Result<bool> {
        let now = OffsetDateTime::now_utc().to_string();
        let updated = self.connection.execute(
            "UPDATE knowledge_spaces
             SET default_permission = ?1, updated_at = ?2
             WHERE id = ?3 AND deleted_at IS NULL",
            params![permission.as_str(), now, space_id],
        )?;

        Ok(updated > 0)
    }

    pub fn list_files(&self, space_id: &str) -> rusqlite::Result<Vec<KnowledgeFile>> {
        let mut statement = self.connection.prepare(
            "SELECT id, relative_path, extension, parse_status
             FROM files
             WHERE space_id = ?1 AND deleted_at IS NULL
             ORDER BY relative_path COLLATE NOCASE",
        )?;

        let files = statement
            .query_map([space_id], |row| {
                let relative_path: String = row.get(1)?;
                let status_value: String = row.get(3)?;
                let status = ParseStatus::from_db(&status_value).unwrap_or(ParseStatus::Failed);

                Ok(KnowledgeFile {
                    id: row.get(0)?,
                    name: display_file_name(&relative_path),
                    extension: display_extension(row.get::<_, String>(2)?),
                    status_label: status.label().to_string(),
                    status,
                })
            })?
            .collect();

        files
    }

    pub fn list_parse_candidates(
        &self,
        space_id: &str,
    ) -> rusqlite::Result<Vec<FileParseCandidate>> {
        let mut statement = self.connection.prepare(
            "SELECT id, relative_path, extension
             FROM files
             WHERE space_id = ?1
               AND deleted_at IS NULL
               AND parse_status IN ('queued', 'changed', 'failed')
             ORDER BY relative_path COLLATE NOCASE",
        )?;

        let candidates = statement
            .query_map([space_id], |row| {
                Ok(FileParseCandidate {
                    file_id: row.get(0)?,
                    relative_path: row.get(1)?,
                    extension: row.get(2)?,
                })
            })?
            .filter_map(|candidate| match candidate {
                Ok(candidate) if is_document_parse_extension(&candidate.extension) => {
                    Some(Ok(candidate))
                }
                Ok(_) => None,
                Err(error) => Some(Err(error)),
            })
            .collect();

        candidates
    }

    fn export_backup_files(&self, space_id: &str) -> rusqlite::Result<Vec<BackupExportFile>> {
        let mut statement = self.connection.prepare(
            "SELECT id, relative_path, extension, content_hash, size_bytes, modified_at,
                    parse_status, last_scanned_at, created_at, updated_at, deleted_at
             FROM files
             WHERE space_id = ?1
             ORDER BY relative_path COLLATE NOCASE, created_at",
        )?;

        let files: rusqlite::Result<Vec<_>> = statement
            .query_map([space_id], |row| {
                Ok(BackupExportFile {
                    id: row.get(0)?,
                    relative_path: row.get(1)?,
                    extension: row.get(2)?,
                    content_hash: row.get(3)?,
                    size_bytes: row.get(4)?,
                    modified_at: row.get(5)?,
                    parse_status: row.get(6)?,
                    last_scanned_at: row.get(7)?,
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                    deleted_at: row.get(10)?,
                })
            })?
            .collect();

        Ok(files?
            .into_iter()
            .filter(|file| !is_sensitive_backup_locator(&file.relative_path))
            .collect())
    }

    fn export_backup_markdown_notes(
        &self,
        space_id: &str,
        exported_file_ids: &HashSet<String>,
    ) -> rusqlite::Result<Vec<BackupExportMarkdownNote>> {
        let mut statement = self.connection.prepare(
            "SELECT id, file_id, relative_path, user_editable, last_generated_hash,
                    created_at, updated_at, deleted_at
             FROM markdown_notes
             WHERE space_id = ?1
             ORDER BY relative_path COLLATE NOCASE, created_at",
        )?;

        let markdown_notes: rusqlite::Result<Vec<_>> = statement
            .query_map([space_id], |row| {
                Ok(BackupExportMarkdownNote {
                    id: row.get(0)?,
                    file_id: row.get(1)?,
                    relative_path: row.get(2)?,
                    user_editable: row.get::<_, i64>(3)? != 0,
                    last_generated_hash: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                    deleted_at: row.get(7)?,
                })
            })?
            .collect();

        Ok(markdown_notes?
            .into_iter()
            .filter(|note| !is_sensitive_backup_locator(&note.relative_path))
            .filter(|note| {
                note.file_id
                    .as_ref()
                    .map(|file_id| exported_file_ids.contains(file_id))
                    .unwrap_or(true)
            })
            .collect())
    }

    fn export_backup_knowledge_blocks(
        &self,
        space_id: &str,
        exported_file_ids: &HashSet<String>,
        exported_note_ids: &HashSet<String>,
    ) -> rusqlite::Result<Vec<BackupExportKnowledgeBlock>> {
        let mut statement = self.connection.prepare(
            "SELECT id, file_id, note_id, title, body, source_kind, source_locator,
                    searchable, created_at, updated_at, deleted_at
             FROM knowledge_blocks
             WHERE space_id = ?1
             ORDER BY created_at, fts_rowid",
        )?;

        let knowledge_blocks: rusqlite::Result<Vec<_>> = statement
            .query_map([space_id], |row| {
                Ok(BackupExportKnowledgeBlock {
                    id: row.get(0)?,
                    file_id: row.get(1)?,
                    note_id: row.get(2)?,
                    title: row.get(3)?,
                    body: row.get(4)?,
                    source_kind: row.get(5)?,
                    source_locator: row.get(6)?,
                    searchable: row.get::<_, i64>(7)? != 0,
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                    deleted_at: row.get(10)?,
                })
            })?
            .collect();

        Ok(knowledge_blocks?
            .into_iter()
            .filter(|block| !is_sensitive_backup_locator(&block.source_locator))
            .filter(|block| {
                block
                    .file_id
                    .as_ref()
                    .map(|file_id| exported_file_ids.contains(file_id))
                    .unwrap_or(true)
            })
            .filter(|block| {
                block
                    .note_id
                    .as_ref()
                    .map(|note_id| exported_note_ids.contains(note_id))
                    .unwrap_or(true)
            })
            .collect())
    }

    fn export_backup_parse_jobs(
        &self,
        space_id: &str,
    ) -> rusqlite::Result<Vec<BackupExportParseJob>> {
        let mut statement = self.connection.prepare(
            "SELECT id, file_id, source_locator, job_type, status, NULL AS error_message,
                    started_at, finished_at, progress_current, progress_total, phase,
                    created_at, updated_at
             FROM parse_jobs
             WHERE space_id = ?1
             ORDER BY created_at DESC",
        )?;

        let parse_jobs = statement
            .query_map([space_id], |row| {
                Ok(BackupExportParseJob {
                    id: row.get(0)?,
                    file_id: row.get(1)?,
                    source_locator: row.get(2)?,
                    job_type: row.get(3)?,
                    status: row.get(4)?,
                    error_message: row.get(5)?,
                    started_at: row.get(6)?,
                    finished_at: row.get(7)?,
                    progress_current: row.get(8)?,
                    progress_total: row.get(9)?,
                    phase: row.get(10)?,
                    created_at: row.get(11)?,
                    updated_at: row.get(12)?,
                })
            })?
            .collect();

        parse_jobs
    }

    fn export_backup_trash_entries(
        &self,
        space_id: &str,
        exported_file_ids: &HashSet<String>,
        exported_note_ids: &HashSet<String>,
        exported_knowledge_block_ids: &HashSet<String>,
    ) -> rusqlite::Result<Vec<BackupExportTrashEntry>> {
        let mut statement = self.connection.prepare(
            "SELECT id, entity_kind, entity_id, display_name, original_locator, deleted_at, restored_at
             FROM trash_entries
             WHERE space_id = ?1
             ORDER BY deleted_at DESC, display_name COLLATE NOCASE",
        )?;

        let trash_entries: rusqlite::Result<Vec<_>> = statement
            .query_map([space_id], |row| {
                Ok(BackupExportTrashEntry {
                    id: row.get(0)?,
                    entity_kind: row.get(1)?,
                    entity_id: row.get(2)?,
                    display_name: row.get(3)?,
                    original_locator: row.get(4)?,
                    deleted_at: row.get(5)?,
                    restored_at: row.get(6)?,
                })
            })?
            .collect();

        Ok(trash_entries?
            .into_iter()
            .filter(|entry| !is_sensitive_backup_locator(&entry.original_locator))
            .filter(|entry| match entry.entity_kind.as_str() {
                "file" => exported_file_ids.contains(&entry.entity_id),
                "markdown_note" => exported_note_ids.contains(&entry.entity_id),
                "knowledge_block" => exported_knowledge_block_ids.contains(&entry.entity_id),
                _ => false,
            })
            .collect())
    }

    pub fn get_file_parse_candidate(
        &self,
        space_id: &str,
        file_id: &str,
    ) -> rusqlite::Result<Option<FileParseCandidate>> {
        self.connection
            .query_row(
                "SELECT id, relative_path, extension
                 FROM files
                 WHERE space_id = ?1 AND id = ?2 AND deleted_at IS NULL",
                params![space_id, file_id],
                |row| {
                    Ok(FileParseCandidate {
                        file_id: row.get(0)?,
                        relative_path: row.get(1)?,
                        extension: row.get(2)?,
                    })
                },
            )
            .optional()
    }

    pub fn replace_file_knowledge_block(
        &mut self,
        space_id: &str,
        file_id: &str,
        document: &ParsedDocument,
    ) -> rusqlite::Result<()> {
        let now = OffsetDateTime::now_utc().to_string();
        let tx = self.connection.transaction()?;
        replace_file_knowledge_block_in_tx(&tx, space_id, file_id, document, &now)?;
        tx.commit()
    }

    pub fn complete_parse_job_if_running(
        &mut self,
        space_id: &str,
        file_id: &str,
        job_id: &str,
        document: &ParsedDocument,
    ) -> rusqlite::Result<ParseJobWriteOutcome> {
        let now = OffsetDateTime::now_utc().to_string();
        let tx = self.connection.transaction()?;
        match parse_job_status_in_tx(&tx, job_id)?.as_deref() {
            Some("running") => {}
            Some("cancelled") => return Ok(ParseJobWriteOutcome::Cancelled),
            _ => return Ok(ParseJobWriteOutcome::NotRunning),
        }
        let job_type = parse_job_type_in_tx(&tx, job_id)?;
        let job_source_locator = parse_job_source_locator_in_tx(&tx, job_id)?;

        if job_type.as_deref() == Some("ocr") {
            if let Some(source_locator) = job_source_locator.as_deref() {
                replace_source_ocr_knowledge_blocks_in_tx(
                    &tx,
                    space_id,
                    file_id,
                    source_locator,
                    document,
                    &now,
                )?;
            } else {
                replace_file_knowledge_block_in_tx(&tx, space_id, file_id, document, &now)?;
            }
        } else {
            replace_file_knowledge_block_in_tx(&tx, space_id, file_id, document, &now)?;
            if job_type.as_deref() == Some("document") {
                enqueue_embedded_image_ocr_jobs_in_tx(&tx, space_id, file_id, document, &now)?;
            }
        }
        tx.execute(
            "UPDATE parse_jobs
             SET status = 'succeeded',
                 error_message = NULL,
                 phase = '已完成',
                 progress_total = CASE WHEN progress_total > 0 THEN progress_total ELSE 1 END,
                 progress_current = CASE WHEN progress_total > 0 THEN progress_total ELSE 1 END,
                 finished_at = ?1,
                 updated_at = ?1
             WHERE id = ?2 AND status = 'running'",
            params![now, job_id],
        )?;
        tx.commit()?;
        Ok(ParseJobWriteOutcome::Updated)
    }

    pub fn mark_file_parse_failed(&self, file_id: &str) -> rusqlite::Result<()> {
        let now = OffsetDateTime::now_utc().to_string();
        mark_file_parse_failed_in_tx(&self.connection, file_id, &now)?;
        Ok(())
    }

    pub fn complete_scan_job_if_running(
        &mut self,
        space_id: &str,
        job_id: &str,
        scanned_files: &[ScannedFile],
    ) -> rusqlite::Result<ScanJobWriteOutcome> {
        let now = OffsetDateTime::now_utc().to_string();
        let tx = self.connection.transaction()?;
        match parse_job_status_in_tx(&tx, job_id)?.as_deref() {
            Some("running") => {}
            Some("cancelled") => return Ok(ScanJobWriteOutcome::Cancelled),
            _ => return Ok(ScanJobWriteOutcome::NotRunning),
        }

        let summary = apply_scan_results_in_tx(&tx, space_id, scanned_files, &now)?;
        let queued_document_count = enqueue_document_parse_jobs_in_tx(&tx, space_id, &now)?;
        let queued_ocr_count = enqueue_image_ocr_parse_jobs_in_tx(&tx, space_id, &now)?;
        tx.execute(
            "UPDATE parse_jobs
             SET status = 'succeeded',
                 error_message = NULL,
                 phase = '已完成',
                 progress_current = CASE WHEN progress_current > 0 THEN progress_current ELSE ?1 END,
                 progress_total = CASE WHEN progress_total > 0 THEN progress_total ELSE ?1 END,
                 finished_at = ?2,
                 updated_at = ?2
             WHERE id = ?3 AND status = 'running'",
            params![scanned_files.len() as i64, now, job_id],
        )?;
        tx.commit()?;

        Ok(ScanJobWriteOutcome::Updated {
            summary,
            queued_document_count,
            queued_ocr_count,
        })
    }

    pub fn fail_space_parse_job_if_running(
        &mut self,
        job_id: &str,
        error_message: &str,
    ) -> rusqlite::Result<ParseJobWriteOutcome> {
        let now = OffsetDateTime::now_utc().to_string();
        let tx = self.connection.transaction()?;
        match parse_job_status_in_tx(&tx, job_id)?.as_deref() {
            Some("running") => {}
            Some("cancelled") => return Ok(ParseJobWriteOutcome::Cancelled),
            _ => return Ok(ParseJobWriteOutcome::NotRunning),
        }

        tx.execute(
            "UPDATE parse_jobs
             SET status = 'failed',
                 error_message = ?1,
                 phase = '失败',
                 finished_at = ?2,
                 updated_at = ?2
             WHERE id = ?3 AND status = 'running'",
            params![error_message, now, job_id],
        )?;
        tx.commit()?;
        Ok(ParseJobWriteOutcome::Updated)
    }

    pub fn fail_parse_job_if_running(
        &mut self,
        file_id: &str,
        job_id: &str,
        error_message: &str,
    ) -> rusqlite::Result<ParseJobWriteOutcome> {
        let now = OffsetDateTime::now_utc().to_string();
        let tx = self.connection.transaction()?;
        match parse_job_status_in_tx(&tx, job_id)?.as_deref() {
            Some("running") => {}
            Some("cancelled") => return Ok(ParseJobWriteOutcome::Cancelled),
            _ => return Ok(ParseJobWriteOutcome::NotRunning),
        }

        mark_file_parse_failed_in_tx(&tx, file_id, &now)?;
        tx.execute(
            "UPDATE parse_jobs
             SET status = 'failed',
                 error_message = ?1,
                 phase = '失败',
                 finished_at = ?2,
                 updated_at = ?2
             WHERE id = ?3 AND status = 'running'",
            params![error_message, now, job_id],
        )?;
        tx.commit()?;
        Ok(ParseJobWriteOutcome::Updated)
    }

    pub fn latest_knowledge_block(
        &self,
        space_id: &str,
    ) -> rusqlite::Result<Option<KnowledgeBlockSearchHit>> {
        self.connection
            .query_row(
                "SELECT id, title, body, source_locator, source_kind
                 FROM knowledge_blocks
                 WHERE space_id = ?1
                   AND searchable = 1
                   AND deleted_at IS NULL
                 ORDER BY updated_at DESC, fts_rowid DESC
                 LIMIT 1",
                [space_id],
                |row| row_to_search_hit(row, ""),
            )
            .optional()
    }

    pub fn latest_table_insight(
        &self,
        space_id: &str,
    ) -> rusqlite::Result<Option<TableInsightPreview>> {
        self.connection
            .query_row(
                "SELECT id, title, body
                 FROM knowledge_blocks
                 WHERE space_id = ?1
                   AND source_kind = 'table'
                   AND searchable = 1
                   AND deleted_at IS NULL
                 ORDER BY updated_at DESC, fts_rowid DESC
                 LIMIT 1",
                [space_id],
                |row| {
                    let body: String = row.get(2)?;
                    Ok(TableInsightPreview {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        description: build_excerpt(&body, ""),
                    })
                },
            )
            .optional()
    }

    pub fn search_knowledge_blocks(
        &self,
        space_id: &str,
        query: &str,
        limit: usize,
    ) -> rusqlite::Result<Vec<KnowledgeBlockSearchHit>> {
        let terms = search_terms(query);
        if terms.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let candidate_limit = limit.saturating_mul(3).max(limit);
        let mut hits = Vec::new();
        for term in &terms {
            append_unique_hits(
                &mut hits,
                self.search_knowledge_blocks_fts(space_id, term, candidate_limit)?,
                candidate_limit,
            );
            if hits.len() >= candidate_limit {
                break;
            }
        }

        if hits.len() < candidate_limit {
            for term in &terms {
                append_unique_hits(
                    &mut hits,
                    self.search_knowledge_blocks_like(space_id, term, candidate_limit)?,
                    candidate_limit,
                );
                if hits.len() >= candidate_limit {
                    break;
                }
            }
        }

        rank_search_hits(&mut hits, query);
        hits.truncate(limit);
        Ok(hits)
    }

    pub fn knowledge_block_context(
        &self,
        space_id: &str,
        block_id: &str,
    ) -> rusqlite::Result<Option<KnowledgeBlockContext>> {
        let current_file_id = self
            .connection
            .query_row(
                "SELECT file_id
                 FROM knowledge_blocks
                 WHERE space_id = ?1
                   AND id = ?2
                   AND searchable = 1
                   AND deleted_at IS NULL",
                params![space_id, block_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?;

        let Some(current_file_id) = current_file_id else {
            return Ok(None);
        };

        let blocks = match current_file_id {
            Some(file_id) => self.knowledge_blocks_for_file(space_id, &file_id)?,
            None => self.knowledge_blocks_for_ids(space_id, &[block_id])?,
        };
        let Some(current_position) = blocks.iter().position(|block| block.id == block_id) else {
            return Ok(None);
        };

        Ok(Some(KnowledgeBlockContext {
            current_index: current_position as u32 + 1,
            total_count: blocks.len() as u32,
            blocks,
        }))
    }

    pub fn enqueue_parse_job(
        &self,
        space_id: &str,
        file_id: &str,
        job_type: &str,
    ) -> rusqlite::Result<String> {
        Ok(self
            .enqueue_parse_job_with_status(space_id, file_id, job_type)?
            .id)
    }

    pub fn enqueue_parse_job_with_status(
        &self,
        space_id: &str,
        file_id: &str,
        job_type: &str,
    ) -> rusqlite::Result<EnqueueParseJobResult> {
        self.enqueue_parse_job_for_source_with_status(space_id, file_id, job_type, None)
    }

    pub fn enqueue_parse_job_for_source_with_status(
        &self,
        space_id: &str,
        file_id: &str,
        job_type: &str,
        source_locator: Option<&str>,
    ) -> rusqlite::Result<EnqueueParseJobResult> {
        let source_locator = normalize_optional_source_locator(source_locator);
        if let Some(existing_id) = self
            .connection
            .query_row(
                "SELECT id
                 FROM parse_jobs
                 WHERE space_id = ?1
                   AND file_id = ?2
                   AND job_type = ?3
                   AND ((?4 IS NULL AND source_locator IS NULL) OR source_locator = ?4)
                   AND status IN ('queued', 'running')
                 ORDER BY created_at DESC
                 LIMIT 1",
                params![space_id, file_id, job_type, source_locator],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            return Ok(EnqueueParseJobResult {
                id: existing_id,
                inserted: false,
            });
        }

        let id = Uuid::new_v4().to_string();
        let now = OffsetDateTime::now_utc().to_string();
        self.connection.execute(
            "INSERT INTO parse_jobs (
                id, space_id, file_id, source_locator, job_type, status, phase,
                progress_current, progress_total, created_at, updated_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5, 'queued', '等待执行', 0, 1, ?6, ?6)",
            params![id, space_id, file_id, source_locator, job_type, now],
        )?;
        Ok(EnqueueParseJobResult { id, inserted: true })
    }

    pub fn enqueue_space_parse_job_with_status(
        &self,
        space_id: &str,
        job_type: &str,
    ) -> rusqlite::Result<EnqueueParseJobResult> {
        if let Some(existing_id) = self
            .connection
            .query_row(
                "SELECT id
                 FROM parse_jobs
                 WHERE space_id = ?1
                   AND file_id IS NULL
                   AND job_type = ?2
                   AND status IN ('queued', 'running')
                 ORDER BY created_at DESC
                 LIMIT 1",
                params![space_id, job_type],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            return Ok(EnqueueParseJobResult {
                id: existing_id,
                inserted: false,
            });
        }

        let id = Uuid::new_v4().to_string();
        let now = OffsetDateTime::now_utc().to_string();
        self.connection.execute(
            "INSERT INTO parse_jobs (
                id, space_id, file_id, job_type, status, phase, progress_current,
                progress_total, created_at, updated_at
             )
             VALUES (?1, ?2, NULL, ?3, 'queued', '等待执行', 0, 0, ?4, ?4)",
            params![id, space_id, job_type, now],
        )?;
        Ok(EnqueueParseJobResult { id, inserted: true })
    }

    pub fn enqueue_document_parse_jobs(&self, space_id: &str) -> rusqlite::Result<u32> {
        let candidates = self.list_parse_candidates(space_id)?;
        let mut inserted_count = 0_u32;

        for candidate in candidates {
            if self
                .enqueue_parse_job_with_status(space_id, &candidate.file_id, "document")?
                .inserted
            {
                inserted_count += 1;
            }
        }

        Ok(inserted_count)
    }

    pub fn enqueue_image_ocr_parse_jobs(&self, space_id: &str) -> rusqlite::Result<u32> {
        let now = OffsetDateTime::now_utc().to_string();
        let candidates = list_image_ocr_candidates_in_tx(&self.connection, space_id)?;
        let mut inserted_count = 0_u32;

        for candidate in candidates {
            if enqueue_file_parse_job_in_tx(
                &self.connection,
                space_id,
                &candidate.file_id,
                "ocr",
                None,
                &now,
            )? {
                inserted_count += 1;
            }
        }

        Ok(inserted_count)
    }

    pub fn has_queued_parse_job(&self, space_id: &str, job_type: &str) -> rusqlite::Result<bool> {
        self.connection.query_row(
            "SELECT EXISTS(
                SELECT 1
                FROM parse_jobs
                WHERE space_id = ?1 AND job_type = ?2 AND status = 'queued'
            )",
            params![space_id, job_type],
            |row| row.get::<_, bool>(0),
        )
    }

    pub fn list_parse_jobs(&self, space_id: &str) -> rusqlite::Result<Vec<ParseJobSummary>> {
        let mut statement = self.connection.prepare(
            "SELECT
                job.id,
                job.file_id,
                job.source_locator,
                CASE
                    WHEN job.file_id IS NULL AND job.job_type = 'scan' THEN '文件夹扫描'
                    ELSE COALESCE(file.relative_path, '未知文件')
                END AS file_name,
                job.job_type,
                job.status,
                job.error_message,
                job.started_at,
                job.finished_at,
                job.progress_current,
                job.progress_total,
                job.phase
             FROM parse_jobs job
             LEFT JOIN files file ON file.id = job.file_id
             WHERE job.space_id = ?1
             ORDER BY job.created_at DESC",
        )?;

        let jobs = statement
            .query_map([space_id], |row| {
                let source_locator: Option<String> = row.get(2)?;
                let relative_path: String = row.get(3)?;
                Ok(ParseJobSummary {
                    id: row.get(0)?,
                    file_id: row.get(1)?,
                    file_name: display_parse_job_file_name(
                        &relative_path,
                        source_locator.as_deref(),
                    ),
                    source_locator,
                    job_type: row.get(4)?,
                    status: row.get(5)?,
                    error_message: row.get(6)?,
                    started_at: row.get(7)?,
                    finished_at: row.get(8)?,
                    progress_current: row.get(9)?,
                    progress_total: row.get(10)?,
                    phase: row.get(11)?,
                })
            })?
            .collect();

        jobs
    }

    pub fn next_queued_parse_job(
        &self,
        space_id: &str,
        job_type: &str,
    ) -> rusqlite::Result<Option<ParseJobCandidate>> {
        self.connection
            .query_row(
                "SELECT job.id, file.id, file.relative_path, file.extension, job.source_locator
                 FROM parse_jobs job
                 JOIN files file ON file.id = job.file_id
                 WHERE job.space_id = ?1
                   AND job.job_type = ?2
                   AND job.status = 'queued'
                   AND file.deleted_at IS NULL
                 ORDER BY job.created_at ASC
                 LIMIT 1",
                params![space_id, job_type],
                |row| {
                    Ok(ParseJobCandidate {
                        job_id: row.get(0)?,
                        file_id: row.get(1)?,
                        relative_path: row.get(2)?,
                        extension: row.get(3)?,
                        source_locator: row.get(4)?,
                    })
                },
            )
            .optional()
    }

    pub fn claim_next_queued_parse_job(
        &mut self,
        space_id: &str,
        job_type: &str,
    ) -> rusqlite::Result<Option<ParseJobCandidate>> {
        let now = OffsetDateTime::now_utc().to_string();
        let tx = self.connection.transaction()?;
        let candidate = tx
            .query_row(
                "SELECT job.id, file.id, file.relative_path, file.extension, job.source_locator
                 FROM parse_jobs job
                 JOIN files file ON file.id = job.file_id
                 WHERE job.space_id = ?1
                   AND job.job_type = ?2
                   AND job.status = 'queued'
                   AND file.deleted_at IS NULL
                 ORDER BY job.created_at ASC
                 LIMIT 1",
                params![space_id, job_type],
                |row| {
                    Ok(ParseJobCandidate {
                        job_id: row.get(0)?,
                        file_id: row.get(1)?,
                        relative_path: row.get(2)?,
                        extension: row.get(3)?,
                        source_locator: row.get(4)?,
                    })
                },
            )
            .optional()?;
        let Some(candidate) = candidate else {
            tx.commit()?;
            return Ok(None);
        };

        let updated = tx.execute(
            "UPDATE parse_jobs
             SET status = 'running',
                 error_message = NULL,
                 started_at = COALESCE(started_at, ?1),
                 finished_at = NULL,
                 phase = '正在准备',
                 progress_current = 0,
                 progress_total = CASE WHEN progress_total > 0 THEN progress_total ELSE 1 END,
                 updated_at = ?1
             WHERE id = ?2 AND status = 'queued'",
            params![now, candidate.job_id],
        )?;
        tx.commit()?;

        if updated > 0 {
            Ok(Some(candidate))
        } else {
            Ok(None)
        }
    }

    pub fn claim_next_queued_space_parse_job(
        &mut self,
        space_id: &str,
        job_type: &str,
    ) -> rusqlite::Result<Option<String>> {
        let now = OffsetDateTime::now_utc().to_string();
        let tx = self.connection.transaction()?;
        let job_id = tx
            .query_row(
                "SELECT id
                 FROM parse_jobs
                 WHERE space_id = ?1
                   AND file_id IS NULL
                   AND job_type = ?2
                   AND status = 'queued'
                 ORDER BY created_at ASC
                 LIMIT 1",
                params![space_id, job_type],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(job_id) = job_id else {
            tx.commit()?;
            return Ok(None);
        };

        let updated = tx.execute(
            "UPDATE parse_jobs
             SET status = 'running',
                 error_message = NULL,
                 started_at = COALESCE(started_at, ?1),
                 finished_at = NULL,
                 phase = '正在准备',
                 progress_current = 0,
                 progress_total = 0,
                 updated_at = ?1
             WHERE id = ?2 AND status = 'queued'",
            params![now, job_id],
        )?;
        tx.commit()?;

        if updated > 0 {
            Ok(Some(job_id))
        } else {
            Ok(None)
        }
    }

    pub fn cancel_parse_job(&self, job_id: &str) -> rusqlite::Result<bool> {
        let now = OffsetDateTime::now_utc().to_string();
        let updated = self.connection.execute(
            "UPDATE parse_jobs
             SET status = 'cancelled',
                 phase = '已取消',
                 finished_at = ?1,
                 updated_at = ?1
             WHERE id = ?2 AND status IN ('queued', 'running')",
            params![now, job_id],
        )?;

        Ok(updated > 0)
    }

    pub fn mark_parse_job_running(&self, job_id: &str) -> rusqlite::Result<bool> {
        let now = OffsetDateTime::now_utc().to_string();
        let updated = self.connection.execute(
            "UPDATE parse_jobs
             SET status = 'running',
                 error_message = NULL,
                 started_at = COALESCE(started_at, ?1),
                 finished_at = NULL,
                 phase = '正在准备',
                 progress_current = 0,
                 progress_total = CASE WHEN progress_total > 0 THEN progress_total ELSE 1 END,
                 updated_at = ?1
             WHERE id = ?2 AND status = 'queued'",
            params![now, job_id],
        )?;

        Ok(updated > 0)
    }

    pub fn update_parse_job_progress(
        &self,
        job_id: &str,
        phase: &str,
        progress_current: u32,
        progress_total: u32,
    ) -> rusqlite::Result<bool> {
        let now = OffsetDateTime::now_utc().to_string();
        let updated = self.connection.execute(
            "UPDATE parse_jobs
             SET phase = ?1,
                 progress_current = ?2,
                 progress_total = ?3,
                 updated_at = ?4
             WHERE id = ?5 AND status = 'running'",
            params![
                phase,
                i64::from(progress_current),
                i64::from(progress_total),
                now,
                job_id
            ],
        )?;

        Ok(updated > 0)
    }

    pub fn mark_parse_job_succeeded(&self, job_id: &str) -> rusqlite::Result<bool> {
        let now = OffsetDateTime::now_utc().to_string();
        let updated = self.connection.execute(
            "UPDATE parse_jobs
             SET status = 'succeeded',
                 error_message = NULL,
                 phase = '已完成',
                 progress_total = CASE WHEN progress_total > 0 THEN progress_total ELSE 1 END,
                 progress_current = CASE WHEN progress_total > 0 THEN progress_total ELSE 1 END,
                 finished_at = ?1,
                 updated_at = ?1
             WHERE id = ?2 AND status = 'running'",
            params![now, job_id],
        )?;

        Ok(updated > 0)
    }

    pub fn mark_parse_job_failed(
        &self,
        job_id: &str,
        error_message: &str,
    ) -> rusqlite::Result<bool> {
        let now = OffsetDateTime::now_utc().to_string();
        let updated = self.connection.execute(
            "UPDATE parse_jobs
             SET status = 'failed',
                 error_message = ?1,
                 phase = '失败',
                 finished_at = ?2,
                 updated_at = ?2
             WHERE id = ?3 AND status = 'running'",
            params![error_message, now, job_id],
        )?;

        Ok(updated > 0)
    }

    pub fn parse_job_status(&self, job_id: &str) -> rusqlite::Result<Option<String>> {
        self.connection
            .query_row(
                "SELECT status FROM parse_jobs WHERE id = ?1",
                [job_id],
                |row| row.get(0),
            )
            .optional()
    }

    pub fn count_knowledge_spaces(&self) -> rusqlite::Result<u32> {
        self.connection.query_row(
            "SELECT COUNT(*) FROM knowledge_spaces WHERE deleted_at IS NULL",
            [],
            |row| row.get::<_, u32>(0),
        )
    }

    fn search_knowledge_blocks_fts(
        &self,
        space_id: &str,
        term: &str,
        limit: usize,
    ) -> rusqlite::Result<Vec<KnowledgeBlockSearchHit>> {
        let mut statement = self.connection.prepare(
            "SELECT block.id, block.title, block.body, block.source_locator, block.source_kind
             FROM knowledge_blocks_fts fts
             JOIN knowledge_blocks block ON block.fts_rowid = fts.rowid
             WHERE block.space_id = ?1
               AND block.searchable = 1
               AND block.deleted_at IS NULL
               AND knowledge_blocks_fts MATCH ?2
             ORDER BY rank
             LIMIT ?3",
        )?;
        let rows = statement.query_map(params![space_id, term, limit as i64], |row| {
            row_to_search_hit(row, term)
        });

        match rows {
            Ok(mapped) => mapped
                .collect::<rusqlite::Result<Vec<_>>>()
                .or_else(|_| Ok(Vec::new())),
            Err(_) => Ok(Vec::new()),
        }
    }

    fn search_knowledge_blocks_like(
        &self,
        space_id: &str,
        term: &str,
        limit: usize,
    ) -> rusqlite::Result<Vec<KnowledgeBlockSearchHit>> {
        let pattern = format!("%{term}%");
        let mut statement = self.connection.prepare(
            "SELECT id, title, body, source_locator, source_kind
             FROM knowledge_blocks
             WHERE space_id = ?1
               AND searchable = 1
               AND deleted_at IS NULL
               AND (title LIKE ?2 OR body LIKE ?2 OR source_locator LIKE ?2)
             ORDER BY updated_at DESC, fts_rowid DESC
             LIMIT ?3",
        )?;

        let hits = statement
            .query_map(params![space_id, pattern, limit as i64], |row| {
                row_to_search_hit(row, term)
            })?
            .collect();

        hits
    }

    fn knowledge_blocks_for_file(
        &self,
        space_id: &str,
        file_id: &str,
    ) -> rusqlite::Result<Vec<KnowledgeBlockSearchHit>> {
        let mut statement = self.connection.prepare(
            "SELECT id, title, body, source_locator, source_kind
             FROM knowledge_blocks
             WHERE space_id = ?1
               AND file_id = ?2
               AND searchable = 1
               AND deleted_at IS NULL
             ORDER BY source_locator COLLATE NOCASE ASC, fts_rowid ASC",
        )?;

        let blocks = statement
            .query_map(params![space_id, file_id], |row| row_to_search_hit(row, ""))?
            .collect();
        blocks
    }

    fn knowledge_blocks_for_ids(
        &self,
        space_id: &str,
        block_ids: &[&str],
    ) -> rusqlite::Result<Vec<KnowledgeBlockSearchHit>> {
        if block_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut statement = self.connection.prepare(
            "SELECT id, title, body, source_locator, source_kind
             FROM knowledge_blocks
             WHERE space_id = ?1
               AND id = ?2
               AND searchable = 1
               AND deleted_at IS NULL
             ORDER BY fts_rowid ASC",
        )?;

        let blocks = statement
            .query_map(params![space_id, block_ids[0]], |row| {
                row_to_search_hit(row, "")
            })?
            .collect();
        blocks
    }

    pub fn apply_scan_results(
        &mut self,
        space_id: &str,
        scanned_files: &[ScannedFile],
    ) -> rusqlite::Result<ScanSummary> {
        let now = OffsetDateTime::now_utc().to_string();
        let tx = self.connection.transaction()?;
        let summary = apply_scan_results_in_tx(&tx, space_id, scanned_files, &now)?;
        tx.commit()?;
        Ok(summary)
    }

    fn foundation_schema_needs_rebuild(&self) -> rusqlite::Result<bool> {
        let spaces_has_inline_unique = self.table_sql("knowledge_spaces")?.map_or(false, |sql| {
            sql.contains("root_path TEXT NOT NULL COLLATE NOCASE UNIQUE")
        });
        let files_has_inline_unique = self.table_sql("files")?.map_or(false, |sql| {
            sql.contains("UNIQUE(space_id, relative_path)")
                || sql.contains("UNIQUE (space_id, relative_path)")
        });
        let blocks_need_stable_rowid = self.table_exists("knowledge_blocks")?
            && !self.column_exists("knowledge_blocks", "fts_rowid")?;

        Ok(spaces_has_inline_unique || files_has_inline_unique || blocks_need_stable_rowid)
    }

    fn rebuild_legacy_foundation_schema(&mut self) -> rusqlite::Result<()> {
        let existing_tables = FOUNDATION_TABLES
            .into_iter()
            .filter_map(|table_name| match self.table_exists(table_name) {
                Ok(true) => Some(Ok(table_name)),
                Ok(false) => None,
                Err(error) => Some(Err(error)),
            })
            .collect::<rusqlite::Result<Vec<_>>>()?;

        self.connection.execute_batch("PRAGMA foreign_keys = OFF")?;
        let tx = self.connection.transaction()?;

        tx.execute_batch(
            "DROP TRIGGER IF EXISTS knowledge_blocks_fts_ai;
             DROP TRIGGER IF EXISTS knowledge_blocks_fts_ad;
             DROP TRIGGER IF EXISTS knowledge_blocks_fts_au;
             DROP TABLE IF EXISTS knowledge_blocks_fts;",
        )?;

        for table_name in existing_tables.iter().rev() {
            tx.execute(
                &format!("ALTER TABLE {table_name} RENAME TO __legacy_{table_name}"),
                [],
            )?;
        }

        tx.execute_batch(FOUNDATION_SCHEMA)?;
        copy_legacy_tables(&tx, &existing_tables)?;

        for table_name in existing_tables {
            tx.execute(&format!("DROP TABLE IF EXISTS __legacy_{table_name}"), [])?;
        }

        tx.commit()?;
        self.connection.execute_batch("PRAGMA foreign_keys = ON")
    }

    fn table_exists(&self, table_name: &str) -> rusqlite::Result<bool> {
        self.connection.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1
            )",
            [table_name],
            |row| row.get::<_, bool>(0),
        )
    }

    fn table_sql(&self, table_name: &str) -> rusqlite::Result<Option<String>> {
        self.connection
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = ?1",
                [table_name],
                |row| row.get(0),
            )
            .optional()
    }

    fn column_exists(&self, table_name: &str, column_name: &str) -> rusqlite::Result<bool> {
        let mut statement = self
            .connection
            .prepare(&format!("PRAGMA table_info({table_name})"))?;
        let mut rows = statement.query([])?;

        while let Some(row) = rows.next()? {
            let name: String = row.get(1)?;
            if name == column_name {
                return Ok(true);
            }
        }

        Ok(false)
    }
}

fn copy_legacy_tables(tx: &Transaction<'_>, table_names: &[&str]) -> rusqlite::Result<()> {
    if table_names.contains(&"knowledge_spaces") {
        tx.execute_batch(
            "INSERT INTO knowledge_spaces (
                id, name, root_path, default_permission, created_at, updated_at, deleted_at
            )
            SELECT id, name, root_path, default_permission, created_at, updated_at, deleted_at
            FROM __legacy_knowledge_spaces;",
        )?;
    }

    if table_names.contains(&"files") {
        tx.execute_batch(
            "INSERT INTO files (
                id, space_id, relative_path, extension, content_hash, modified_at, parse_status,
                created_at, updated_at, deleted_at
            )
            SELECT id, space_id, relative_path, extension, content_hash, modified_at, parse_status,
                created_at, updated_at, deleted_at
            FROM __legacy_files;",
        )?;
    }

    if table_names.contains(&"markdown_notes") {
        tx.execute_batch(
            "INSERT INTO markdown_notes (
                id, file_id, space_id, relative_path, user_editable, last_generated_hash,
                created_at, updated_at, deleted_at
            )
            SELECT id, file_id, space_id, relative_path, user_editable, last_generated_hash,
                created_at, updated_at, deleted_at
            FROM __legacy_markdown_notes;",
        )?;
    }

    if table_names.contains(&"knowledge_blocks") {
        tx.execute_batch(
            "INSERT INTO knowledge_blocks (
                id, space_id, file_id, note_id, title, body, source_kind, source_locator,
                searchable, created_at, updated_at, deleted_at
            )
            SELECT id, space_id, file_id, note_id, title, body, source_kind, source_locator,
                searchable, created_at, updated_at, deleted_at
            FROM __legacy_knowledge_blocks;",
        )?;
    }

    if table_names.contains(&"parse_jobs") {
        tx.execute_batch(
            "INSERT INTO parse_jobs (
                id, space_id, file_id, job_type, status, error_message, created_at, updated_at
            )
            SELECT id, space_id, file_id, job_type, status, error_message, created_at, updated_at
            FROM __legacy_parse_jobs;",
        )?;
    }

    if table_names.contains(&"trash_entries") {
        tx.execute_batch(
            "INSERT INTO trash_entries (
                id, space_id, entity_kind, entity_id, display_name, original_locator,
                deleted_at, restored_at
            )
            SELECT id, space_id, entity_kind, entity_id, display_name, original_locator,
                deleted_at, restored_at
            FROM __legacy_trash_entries;",
        )?;
    }

    Ok(())
}

fn apply_scan_results_in_tx(
    tx: &Transaction<'_>,
    space_id: &str,
    scanned_files: &[ScannedFile],
    now: &str,
) -> rusqlite::Result<ScanSummary> {
    let scan_run_id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO scan_runs (id, space_id, started_at, status)
         VALUES (?1, ?2, ?3, 'running')",
        params![scan_run_id, space_id, now],
    )?;

    let existing_files = load_existing_files(tx, space_id)?;
    let mut seen_keys = Vec::with_capacity(scanned_files.len());
    let mut summary = ScanSummary::default();

    for scanned_file in scanned_files {
        let key = normalize_lookup_key(&scanned_file.relative_path);
        seen_keys.push(key.clone());

        match existing_files.iter().find(|file| file.lookup_key == key) {
            Some(existing) if existing.deleted_at.is_none() => {
                let changed = existing.content_hash.as_deref()
                    != Some(scanned_file.content_hash.as_str())
                    || existing.modified_at.as_deref() != Some(scanned_file.modified_at.as_str())
                    || existing.size_bytes != scanned_file.size_bytes;
                let status = if changed {
                    summary.changed_count += 1;
                    ParseStatus::Changed
                } else {
                    existing.parse_status.clone()
                };

                tx.execute(
                    "UPDATE files
                     SET extension = ?1, content_hash = ?2, size_bytes = ?3,
                         modified_at = ?4, parse_status = ?5, last_scanned_at = ?6,
                         updated_at = ?6, deleted_at = NULL
                     WHERE id = ?7",
                    params![
                        scanned_file.extension,
                        scanned_file.content_hash,
                        scanned_file.size_bytes,
                        scanned_file.modified_at,
                        status.as_str(),
                        now,
                        existing.id
                    ],
                )?;
            }
            Some(existing) => {
                summary.added_count += 1;
                tx.execute(
                    "UPDATE files
                     SET relative_path = ?1, extension = ?2, content_hash = ?3,
                         size_bytes = ?4, modified_at = ?5, parse_status = ?6,
                         last_scanned_at = ?7, updated_at = ?7, deleted_at = NULL
                     WHERE id = ?8",
                    params![
                        scanned_file.relative_path,
                        scanned_file.extension,
                        scanned_file.content_hash,
                        scanned_file.size_bytes,
                        scanned_file.modified_at,
                        ParseStatus::Queued.as_str(),
                        now,
                        existing.id
                    ],
                )?;
            }
            None => {
                summary.added_count += 1;
                tx.execute(
                    "INSERT INTO files (
                        id, space_id, relative_path, extension, content_hash, size_bytes,
                        modified_at, parse_status, last_scanned_at, created_at, updated_at
                     )
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, ?9)",
                    params![
                        Uuid::new_v4().to_string(),
                        space_id,
                        scanned_file.relative_path,
                        scanned_file.extension,
                        scanned_file.content_hash,
                        scanned_file.size_bytes,
                        scanned_file.modified_at,
                        ParseStatus::Queued.as_str(),
                        now
                    ],
                )?;
            }
        }
    }

    for existing in existing_files
        .iter()
        .filter(|file| file.deleted_at.is_none() && !seen_keys.contains(&file.lookup_key))
    {
        summary.deleted_count += 1;
        tx.execute(
            "UPDATE files
             SET deleted_at = ?1, updated_at = ?1
             WHERE id = ?2",
            params![now, existing.id],
        )?;
        tx.execute(
            "UPDATE parse_jobs
             SET status = 'cancelled',
                 phase = '已取消',
                 finished_at = ?1,
                 updated_at = ?1
             WHERE file_id = ?2 AND status IN ('queued', 'running')",
            params![now, existing.id],
        )?;
    }

    tx.execute(
        "UPDATE scan_runs
         SET finished_at = ?1, status = 'succeeded', added_count = ?2,
             changed_count = ?3, deleted_count = ?4, failed_count = ?5,
             message = ?6
         WHERE id = ?7",
        params![
            now,
            summary.added_count,
            summary.changed_count,
            summary.deleted_count,
            summary.failed_count,
            format!(
                "新增 {} 个，变更 {} 个，删除 {} 个",
                summary.added_count, summary.changed_count, summary.deleted_count
            ),
            scan_run_id
        ],
    )?;

    Ok(summary)
}

fn enqueue_document_parse_jobs_in_tx(
    tx: &Transaction<'_>,
    space_id: &str,
    now: &str,
) -> rusqlite::Result<u32> {
    let candidates = list_parse_candidates_in_tx(tx, space_id)?;
    let mut inserted_count = 0_u32;

    for candidate in candidates {
        if enqueue_file_parse_job_in_tx(tx, space_id, &candidate.file_id, "document", None, now)? {
            inserted_count += 1;
        }
    }

    Ok(inserted_count)
}

fn enqueue_image_ocr_parse_jobs_in_tx(
    tx: &Transaction<'_>,
    space_id: &str,
    now: &str,
) -> rusqlite::Result<u32> {
    let candidates = list_image_ocr_candidates_in_tx(tx, space_id)?;
    let mut inserted_count = 0_u32;

    for candidate in candidates {
        if enqueue_file_parse_job_in_tx(tx, space_id, &candidate.file_id, "ocr", None, now)? {
            inserted_count += 1;
        }
    }

    Ok(inserted_count)
}

fn enqueue_embedded_image_ocr_jobs_in_tx(
    tx: &Transaction<'_>,
    space_id: &str,
    file_id: &str,
    document: &ParsedDocument,
    now: &str,
) -> rusqlite::Result<u32> {
    let mut inserted_count = 0_u32;
    for segment in document.segments.iter().take(MAX_DOCUMENT_SEGMENTS) {
        if !is_embedded_image_segment(segment) {
            continue;
        }
        if enqueue_file_parse_job_in_tx(
            tx,
            space_id,
            file_id,
            "ocr",
            Some(&segment.source_locator),
            now,
        )? {
            inserted_count += 1;
        }
    }

    Ok(inserted_count)
}

fn is_embedded_image_segment(segment: &ParsedDocumentSegment) -> bool {
    matches!(
        segment
            .evidence
            .as_ref()
            .and_then(|evidence| evidence.kind.as_deref()),
        Some("embedded_image")
    ) && embedded_image_number_from_locator(&segment.source_locator).is_some()
}

fn enqueue_file_parse_job_in_tx(
    connection: &Connection,
    space_id: &str,
    file_id: &str,
    job_type: &str,
    source_locator: Option<&str>,
    now: &str,
) -> rusqlite::Result<bool> {
    let source_locator = normalize_optional_source_locator(source_locator);
    let existing_id = connection
        .query_row(
            "SELECT id
             FROM parse_jobs
             WHERE space_id = ?1
               AND file_id = ?2
               AND job_type = ?3
               AND ((?4 IS NULL AND source_locator IS NULL) OR source_locator = ?4)
               AND status IN ('queued', 'running')
             ORDER BY created_at DESC
             LIMIT 1",
            params![space_id, file_id, job_type, source_locator],
            |row| row.get::<_, String>(0),
        )
        .optional()?;

    if existing_id.is_some() {
        return Ok(false);
    }

    connection.execute(
        "INSERT INTO parse_jobs (
            id, space_id, file_id, source_locator, job_type, status, phase,
            progress_current, progress_total, created_at, updated_at
         )
         VALUES (?1, ?2, ?3, ?4, ?5, 'queued', '等待执行', 0, 1, ?6, ?6)",
        params![
            Uuid::new_v4().to_string(),
            space_id,
            file_id,
            source_locator,
            job_type,
            now
        ],
    )?;

    Ok(true)
}

fn normalize_optional_source_locator(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn list_parse_candidates_in_tx(
    tx: &Transaction<'_>,
    space_id: &str,
) -> rusqlite::Result<Vec<FileParseCandidate>> {
    let mut statement = tx.prepare(
        "SELECT id, relative_path, extension
         FROM files
         WHERE space_id = ?1
           AND deleted_at IS NULL
           AND parse_status IN ('queued', 'changed', 'failed')
         ORDER BY relative_path COLLATE NOCASE",
    )?;

    let candidates = statement
        .query_map([space_id], |row| {
            Ok(FileParseCandidate {
                file_id: row.get(0)?,
                relative_path: row.get(1)?,
                extension: row.get(2)?,
            })
        })?
        .filter_map(|candidate| match candidate {
            Ok(candidate) if is_document_parse_extension(&candidate.extension) => {
                Some(Ok(candidate))
            }
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .collect();

    candidates
}

fn list_image_ocr_candidates_in_tx(
    connection: &Connection,
    space_id: &str,
) -> rusqlite::Result<Vec<FileParseCandidate>> {
    let mut statement = connection.prepare(
        "SELECT id, relative_path, extension
         FROM files
         WHERE space_id = ?1
           AND deleted_at IS NULL
           AND parse_status IN ('queued', 'changed', 'failed')
         ORDER BY relative_path COLLATE NOCASE",
    )?;

    let candidates = statement
        .query_map([space_id], |row| {
            Ok(FileParseCandidate {
                file_id: row.get(0)?,
                relative_path: row.get(1)?,
                extension: row.get(2)?,
            })
        })?
        .filter_map(|candidate| match candidate {
            Ok(candidate) if is_auto_ocr_extension(&candidate.extension) => Some(Ok(candidate)),
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .collect();

    candidates
}

fn is_document_parse_extension(extension: &str) -> bool {
    let extension = extension.trim_start_matches('.').to_lowercase();
    DOCUMENT_PARSE_EXTENSIONS.contains(&extension.as_str())
}

fn is_auto_ocr_extension(extension: &str) -> bool {
    let extension = extension.trim_start_matches('.').to_lowercase();
    AUTO_OCR_EXTENSIONS.contains(&extension.as_str())
}

fn replace_file_knowledge_block_in_tx(
    tx: &Transaction<'_>,
    space_id: &str,
    file_id: &str,
    document: &ParsedDocument,
    now: &str,
) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE knowledge_blocks
         SET searchable = 0, deleted_at = ?1, updated_at = ?1
         WHERE space_id = ?2 AND file_id = ?3 AND deleted_at IS NULL",
        params![now, space_id, file_id],
    )?;

    let blocks = document_knowledge_blocks(document);
    for block in blocks.iter().rev() {
        insert_knowledge_block_in_tx(
            tx,
            space_id,
            file_id,
            &block.title,
            &block.body,
            "original_file",
            &block.source_locator,
            now,
        )?;
    }

    for insight in document.table_insights.iter().rev() {
        insert_knowledge_block_in_tx(
            tx,
            space_id,
            file_id,
            &insight.title,
            &insight.body,
            "table",
            &insight.source_locator,
            now,
        )?;
    }

    tx.execute(
        "UPDATE files
         SET parse_status = ?1, updated_at = ?2
         WHERE id = ?3 AND space_id = ?4 AND deleted_at IS NULL",
        params![ParseStatus::Indexed.as_str(), now, file_id, space_id],
    )?;

    Ok(())
}

fn replace_source_ocr_knowledge_blocks_in_tx(
    tx: &Transaction<'_>,
    space_id: &str,
    file_id: &str,
    source_locator: &str,
    document: &ParsedDocument,
    now: &str,
) -> rusqlite::Result<()> {
    let ocr_source_prefix = format!("{source_locator}#ocr");
    let prefix_len = i64::try_from(ocr_source_prefix.len()).unwrap_or(i64::MAX);
    tx.execute(
        "UPDATE knowledge_blocks
         SET searchable = 0, deleted_at = ?1, updated_at = ?1
         WHERE space_id = ?2
           AND file_id = ?3
           AND deleted_at IS NULL
           AND substr(source_locator, 1, ?4) = ?5",
        params![now, space_id, file_id, prefix_len, ocr_source_prefix],
    )?;

    for block in document_knowledge_blocks(document).iter().rev() {
        insert_knowledge_block_in_tx(
            tx,
            space_id,
            file_id,
            &block.title,
            &block.body,
            "original_file",
            &block.source_locator,
            now,
        )?;
    }

    Ok(())
}

fn insert_knowledge_block_in_tx(
    tx: &Transaction<'_>,
    space_id: &str,
    file_id: &str,
    title: &str,
    body: &str,
    source_kind: &str,
    source_locator: &str,
    now: &str,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO knowledge_blocks (
            id, space_id, file_id, title, body, source_kind, source_locator,
            searchable, created_at, updated_at
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?8)",
        params![
            Uuid::new_v4().to_string(),
            space_id,
            file_id,
            title,
            body,
            source_kind,
            source_locator,
            now
        ],
    )?;

    Ok(())
}

fn document_knowledge_blocks(document: &ParsedDocument) -> Vec<DocumentKnowledgeBlock> {
    if !document.segments.is_empty() {
        let blocks = document
            .segments
            .iter()
            .take(MAX_DOCUMENT_SEGMENTS)
            .scan(MAX_DOCUMENT_SEGMENT_CHARS, |remaining_chars, segment| {
                if *remaining_chars == 0 {
                    return None;
                }

                let bounded_body = take_chars(&segment.body, *remaining_chars);
                *remaining_chars = remaining_chars.saturating_sub(bounded_body.chars().count());
                let evidence_summary = segment
                    .evidence
                    .as_ref()
                    .and_then(evidence_metadata_summary);
                Some(segmented_knowledge_blocks_with_evidence(
                    &segment.title,
                    &bounded_body,
                    &segment.source_locator,
                    evidence_summary.as_deref(),
                ))
            })
            .flatten()
            .collect::<Vec<_>>();
        if !blocks.is_empty() {
            return blocks;
        }
    }

    segmented_knowledge_blocks(&document.title, &document.body, &document.source_locator)
}

fn take_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn segmented_knowledge_blocks(
    title: &str,
    body: &str,
    source_locator: &str,
) -> Vec<DocumentKnowledgeBlock> {
    segmented_knowledge_blocks_with_evidence(title, body, source_locator, None)
}

fn segmented_knowledge_blocks_with_evidence(
    title: &str,
    body: &str,
    source_locator: &str,
    evidence_summary: Option<&str>,
) -> Vec<DocumentKnowledgeBlock> {
    let chunks = split_knowledge_block_body(body);
    let total = chunks.len();

    chunks
        .into_iter()
        .enumerate()
        .map(|(index, body)| {
            let block_number = index + 1;
            let title = if total == 1 {
                title.to_string()
            } else {
                format!("{title} · 片段 {block_number}/{total}")
            };
            let source_locator = if total == 1 {
                source_locator.to_string()
            } else {
                append_source_block_locator(source_locator, block_number)
            };

            let body = body.trim().to_string();
            let body = match evidence_summary.filter(|summary| !summary.trim().is_empty()) {
                Some(summary) => format!("证据范围：{}\n正文：{}", summary.trim(), body),
                None => body,
            };

            DocumentKnowledgeBlock {
                title,
                body,
                source_locator,
            }
        })
        .collect()
}

fn evidence_metadata_summary(evidence: &ParsedEvidenceMetadata) -> Option<String> {
    let mut parts = Vec::new();
    let source_label = match evidence.kind.as_deref() {
        Some("ocr_page") => "OCR",
        Some("pdf_page") => "PDF",
        Some("table_section") => "表格",
        Some("embedded_image") => "文档图片",
        _ => "文档",
    };

    if let Some(image_number) = evidence.image_number {
        parts.push(format!("{source_label} {image_number}"));
    } else if let Some(page_number) = evidence.page_number {
        if let Some(page_count) = evidence.page_count.filter(|count| *count > 0) {
            parts.push(format!("{source_label} 第 {page_number}/{page_count} 页"));
        } else {
            parts.push(format!("{source_label} 第 {page_number} 页"));
        }
    } else if let Some(page_count) = evidence.page_count.filter(|count| *count > 0) {
        parts.push(format!("{source_label} 共 {page_count} 页"));
    }

    if let Some(line_count) = evidence.line_count.filter(|count| *count > 0) {
        parts.push(format!("{line_count} 行"));
    }
    if let Some(char_count) = evidence.char_count.filter(|count| *count > 0) {
        parts.push(format!("{char_count} 字"));
    }
    if let Some(confidence_percent) = evidence.confidence_percent {
        parts.push(format!("置信度 {}%", confidence_percent.min(100)));
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

fn split_knowledge_block_body(body: &str) -> Vec<String> {
    let normalized = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= KNOWLEDGE_BLOCK_MAX_CHARS {
        return vec![normalized];
    }

    let characters = normalized.chars().collect::<Vec<_>>();
    let mut chunks = Vec::new();
    let mut start = 0_usize;
    while start < characters.len() {
        let target_end = (start + KNOWLEDGE_BLOCK_MAX_CHARS).min(characters.len());
        let end = if target_end >= characters.len() {
            target_end
        } else {
            find_chunk_boundary(&characters, start, target_end)
        };
        let chunk = characters[start..end]
            .iter()
            .collect::<String>()
            .trim()
            .to_string();
        if !chunk.is_empty() {
            chunks.push(chunk);
        }

        start = end;
        while start < characters.len() && characters[start].is_whitespace() {
            start += 1;
        }
    }

    if chunks.is_empty() {
        vec![normalized]
    } else {
        chunks
    }
}

fn find_chunk_boundary(characters: &[char], start: usize, target_end: usize) -> usize {
    let min_end = (start + KNOWLEDGE_BLOCK_MIN_SPLIT_CHARS).min(target_end);
    for index in (min_end..target_end).rev() {
        if is_chunk_boundary(characters[index]) {
            return index + 1;
        }
    }

    target_end
}

fn is_chunk_boundary(character: char) -> bool {
    character.is_whitespace()
        || matches!(
            character,
            '。' | '！' | '？' | '；' | '.' | '!' | '?' | ';' | '\n' | '\r'
        )
}

fn append_source_block_locator(source_locator: &str, block_number: usize) -> String {
    let trimmed = source_locator.trim();
    if trimmed.ends_with("#ocr") {
        let source_path = strip_known_source_fragment(trimmed);
        return format!("{source_path}#ocr-block-{block_number:03}");
    }

    if source_locator_has_page_fragment(trimmed) {
        return format!("{trimmed}#block-{block_number:03}");
    }

    let source_path = strip_known_source_fragment(trimmed);
    format!("{source_path}#block-{block_number:03}")
}

fn source_locator_has_page_fragment(source_locator: &str) -> bool {
    source_locator.split('#').skip(1).any(|fragment| {
        numbered_fragment(fragment, "page-")
            || numbered_fragment(fragment, "ocr-page-")
            || numbered_fragment(fragment, "image-")
    })
}

fn mark_file_parse_failed_in_tx(
    connection: &Connection,
    file_id: &str,
    now: &str,
) -> rusqlite::Result<()> {
    connection.execute(
        "UPDATE files
         SET parse_status = ?1, updated_at = ?2
         WHERE id = ?3 AND deleted_at IS NULL",
        params![ParseStatus::Failed.as_str(), now, file_id],
    )?;
    Ok(())
}

fn parse_job_status_in_tx(tx: &Transaction<'_>, job_id: &str) -> rusqlite::Result<Option<String>> {
    tx.query_row(
        "SELECT status FROM parse_jobs WHERE id = ?1",
        [job_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
}

fn parse_job_type_in_tx(tx: &Transaction<'_>, job_id: &str) -> rusqlite::Result<Option<String>> {
    tx.query_row(
        "SELECT job_type FROM parse_jobs WHERE id = ?1",
        [job_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
}

fn parse_job_source_locator_in_tx(
    tx: &Transaction<'_>,
    job_id: &str,
) -> rusqlite::Result<Option<String>> {
    tx.query_row(
        "SELECT source_locator FROM parse_jobs WHERE id = ?1",
        [job_id],
        |row| row.get::<_, Option<String>>(0),
    )
    .optional()
    .map(|source_locator| source_locator.flatten())
}

#[derive(Debug)]
struct ExistingFile {
    id: String,
    lookup_key: String,
    content_hash: Option<String>,
    modified_at: Option<String>,
    size_bytes: i64,
    parse_status: ParseStatus,
    deleted_at: Option<String>,
}

fn load_existing_files(
    tx: &Transaction<'_>,
    space_id: &str,
) -> rusqlite::Result<Vec<ExistingFile>> {
    let mut statement = tx.prepare(
        "SELECT id, relative_path, content_hash, modified_at, size_bytes, parse_status, deleted_at
         FROM files
         WHERE space_id = ?1",
    )?;

    let files = statement
        .query_map([space_id], |row| {
            let relative_path: String = row.get(1)?;
            let parse_status: String = row.get(5)?;
            Ok(ExistingFile {
                id: row.get(0)?,
                lookup_key: normalize_lookup_key(&relative_path),
                content_hash: row.get(2)?,
                modified_at: row.get(3)?,
                size_bytes: row.get(4)?,
                parse_status: ParseStatus::from_db(&parse_status).unwrap_or(ParseStatus::Failed),
                deleted_at: row.get(6)?,
            })
        })?
        .collect();

    files
}

fn normalize_lookup_key(relative_path: &str) -> String {
    relative_path.replace('/', "\\").to_lowercase()
}

fn row_to_search_hit(row: &Row<'_>, term: &str) -> rusqlite::Result<KnowledgeBlockSearchHit> {
    let body: String = row.get(2)?;
    let source_locator: String = row.get(3)?;
    let source_kind = row.get::<_, String>(4)?;
    let source_kind = if is_ocr_source_locator(&source_locator) {
        "ocr".to_string()
    } else {
        source_kind
    };

    Ok(KnowledgeBlockSearchHit {
        id: row.get(0)?,
        title: row.get(1)?,
        excerpt: build_excerpt(&body, term),
        source_file_name: display_source_file_name(&source_locator),
        source_locator,
        source_kind,
    })
}

fn rank_search_hits(hits: &mut [KnowledgeBlockSearchHit], query: &str) {
    let normalized_query = query.to_lowercase();
    let wants_ocr = query_contains_ocr_intent(&normalized_query);
    let original_positions: HashMap<String, usize> = hits
        .iter()
        .enumerate()
        .map(|(index, hit)| (hit.id.clone(), index))
        .collect();

    hits.sort_by(|left, right| {
        let left_position = original_positions
            .get(&left.id)
            .copied()
            .unwrap_or(usize::MAX);
        let right_position = original_positions
            .get(&right.id)
            .copied()
            .unwrap_or(usize::MAX);
        let left_score = source_rank_score(left, wants_ocr);
        let right_score = source_rank_score(right, wants_ocr);

        adjusted_source_position(left_position, left_score)
            .cmp(&adjusted_source_position(right_position, right_score))
            .then_with(|| right_score.cmp(&left_score))
            .then_with(|| left_position.cmp(&right_position))
    });
}

fn source_rank_score(hit: &KnowledgeBlockSearchHit, wants_ocr: bool) -> u8 {
    match hit.source_kind.as_str() {
        "ocr" if wants_ocr => 1,
        "table" => 1,
        _ => 0,
    }
}

fn adjusted_source_position(position: usize, source_score: u8) -> usize {
    position.saturating_sub(source_score as usize)
}

fn query_contains_ocr_intent(query: &str) -> bool {
    ["ocr", "扫描", "扫描版", "图片", "截图", "识别"]
        .iter()
        .any(|token| query.contains(token))
}

fn append_unique_hits(
    current: &mut Vec<KnowledgeBlockSearchHit>,
    incoming: Vec<KnowledgeBlockSearchHit>,
    limit: usize,
) {
    for hit in incoming {
        if current.iter().any(|existing| existing.id == hit.id) {
            continue;
        }

        current.push(hit);
        if current.len() >= limit {
            break;
        }
    }
}

fn search_terms(query: &str) -> Vec<String> {
    let normalized = query.trim();
    if normalized.is_empty() {
        return Vec::new();
    }

    let mut terms = Vec::new();
    push_unique_term(&mut terms, normalized);

    for part in normalized
        .split(|character: char| character.is_whitespace() || is_query_punctuation(character))
    {
        push_unique_term(&mut terms, part);
        let characters = part.chars().collect::<Vec<_>>();
        for window_size in [4_usize, 3_usize] {
            if characters.len() <= window_size {
                continue;
            }

            for window in characters.windows(window_size).take(8) {
                let term = window.iter().collect::<String>();
                push_unique_term(&mut terms, &term);
            }
        }
    }

    terms
}

fn push_unique_term(terms: &mut Vec<String>, value: &str) {
    let term = value.trim_matches(is_query_punctuation).trim();
    if term.chars().count() < 2 || terms.iter().any(|existing| existing == term) {
        return;
    }

    terms.push(term.to_string());
}

fn is_query_punctuation(character: char) -> bool {
    matches!(
        character,
        '，' | '。'
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
            | '"'
            | '\''
            | '“'
            | '”'
            | '‘'
            | '’'
            | '('
            | ')'
            | '（'
            | '）'
    )
}

fn build_excerpt(body: &str, term: &str) -> String {
    let evidence_prefix = body.lines().find_map(|line| {
        let trimmed = line.trim();
        if trimmed.starts_with("证据范围：") {
            Some(trimmed.split_whitespace().collect::<Vec<_>>().join(" "))
        } else {
            None
        }
    });
    let normalized = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut excerpt = if !term.is_empty() {
        normalized
            .find(term)
            .map(|index| normalized[index..].to_string())
            .unwrap_or_else(|| normalized.clone())
    } else {
        normalized
    };
    if let Some(evidence_prefix) = evidence_prefix {
        if !excerpt.starts_with(&evidence_prefix) {
            excerpt = format!("{evidence_prefix} 正文摘录：{excerpt}");
        }
    }

    let mut output = excerpt.chars().take(180).collect::<String>();
    if excerpt.chars().count() > 180 {
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

fn display_parse_job_file_name(relative_path: &str, source_locator: Option<&str>) -> String {
    if let Some(source_locator) = source_locator {
        if let Some(image_number) = embedded_image_number_from_locator(source_locator) {
            return format!(
                "{} · 文档图片 {}",
                display_file_name(strip_known_source_fragment(source_locator)),
                image_number
            );
        }
    }

    display_file_name(relative_path)
}

fn display_source_file_name(source_locator: &str) -> String {
    let source_path = strip_known_source_fragment(source_locator);
    display_file_name(source_path)
}

fn is_ocr_source_locator(source_locator: &str) -> bool {
    source_locator.split('#').skip(1).any(|fragment| {
        fragment == "ocr"
            || numbered_fragment(fragment, "ocr-block-")
            || numbered_fragment(fragment, "ocr-page-")
    })
}

fn is_sensitive_backup_locator(locator: &str) -> bool {
    let source_path = strip_known_source_fragment(locator).trim();
    if source_path.is_empty() {
        return false;
    }

    let normalized = source_path.replace('/', "\\").to_lowercase();
    let parts = normalized
        .split('\\')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    if parts
        .iter()
        .any(|part| *part == ".env" || part.starts_with(".env."))
    {
        return true;
    }

    if parts.iter().any(|part| *part == "library-ocr-runs") {
        return true;
    }

    parts
        .windows(2)
        .any(|window| window[0] == "models" && window[1] == "ocr")
}

fn strip_known_source_fragment(source_locator: &str) -> &str {
    let mut source_path = source_locator.trim();

    while let Some((path, fragment)) = source_path.rsplit_once('#') {
        if !is_known_source_fragment(fragment) {
            break;
        }
        source_path = path.trim();
    }

    source_path
}

fn is_known_source_fragment(fragment: &str) -> bool {
    fragment == "ocr"
        || numbered_fragment(fragment, "page-")
        || numbered_fragment(fragment, "ocr-page-")
        || numbered_fragment(fragment, "image-")
        || numbered_fragment(fragment, "block-")
        || numbered_fragment(fragment, "ocr-block-")
        || numbered_fragment(fragment, "sheet-")
}

fn embedded_image_number_from_locator(source_locator: &str) -> Option<u32> {
    source_locator
        .split('#')
        .skip(1)
        .find_map(|fragment| numbered_fragment_value(fragment, "image-"))
}

fn numbered_fragment(fragment: &str, prefix: &str) -> bool {
    numbered_fragment_value(fragment, prefix).is_some()
}

fn numbered_fragment_value(fragment: &str, prefix: &str) -> Option<u32> {
    let value = fragment.strip_prefix(prefix)?;
    if value.is_empty() || !value.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }
    value.parse::<u32>().ok()
}

fn display_extension(extension: String) -> String {
    if extension.starts_with('.') {
        extension
    } else {
        format!(".{extension}")
    }
}

#[cfg(test)]
mod tests {
    use super::{display_source_file_name, rank_search_hits, ParseJobWriteOutcome, SqliteStore};
    use crate::models::{
        BackupExport, BackupExportFile, BackupExportKnowledgeBlock, BackupExportSpace,
        BackupExportWorkspace, KnowledgeBlockSearchHit, ParsedDocument, ParsedDocumentSegment,
        ParsedEvidenceMetadata, ParsedTableInsight, PermissionMode, ScannedFile,
    };
    use rusqlite::{params, Connection};

    const TEST_TIME: &str = "2026-06-21T00:00:00Z";
    const LEGACY_FOUNDATION_SCHEMA: &str = r#"
        PRAGMA foreign_keys = ON;

        CREATE TABLE knowledge_spaces (
          id TEXT PRIMARY KEY,
          name TEXT NOT NULL,
          root_path TEXT NOT NULL COLLATE NOCASE UNIQUE,
          default_permission TEXT NOT NULL CHECK (default_permission IN ('readonly', 'approval', 'full')),
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          deleted_at TEXT
        );

        CREATE TABLE files (
          id TEXT PRIMARY KEY,
          space_id TEXT NOT NULL REFERENCES knowledge_spaces(id),
          relative_path TEXT NOT NULL COLLATE NOCASE,
          extension TEXT NOT NULL,
          content_hash TEXT,
          modified_at TEXT,
          parse_status TEXT NOT NULL CHECK (parse_status IN ('indexed', 'changed', 'queued', 'failed')),
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          deleted_at TEXT,
          UNIQUE(space_id, relative_path)
        );

        CREATE TABLE knowledge_blocks (
          id TEXT PRIMARY KEY,
          space_id TEXT NOT NULL REFERENCES knowledge_spaces(id),
          file_id TEXT REFERENCES files(id),
          note_id TEXT,
          title TEXT NOT NULL,
          body TEXT NOT NULL,
          source_kind TEXT NOT NULL CHECK (source_kind IN ('original_file', 'markdown_note', 'table')),
          source_locator TEXT NOT NULL,
          searchable INTEGER NOT NULL DEFAULT 1,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          deleted_at TEXT
        );

        CREATE VIRTUAL TABLE knowledge_blocks_fts USING fts5(
          title,
          body,
          content='knowledge_blocks',
          content_rowid='rowid',
          tokenize='trigram'
        );
    "#;

    fn ranked_hit(id: &str, source_kind: &str, source_locator: &str) -> KnowledgeBlockSearchHit {
        KnowledgeBlockSearchHit {
            id: id.to_string(),
            title: id.to_string(),
            excerpt: id.to_string(),
            source_file_name: display_source_file_name(source_locator),
            source_locator: source_locator.to_string(),
            source_kind: source_kind.to_string(),
        }
    }

    #[test]
    fn creates_knowledge_space_in_local_sqlite() {
        let store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let id = store
            .create_knowledge_space("面试", "D:\\知识库\\面试", PermissionMode::Approval)
            .expect("space is inserted");

        assert!(!id.is_empty());
        assert_eq!(store.count_knowledge_spaces().unwrap(), 1);
    }

    #[test]
    fn rejects_case_only_duplicate_root_paths() {
        let store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        store
            .create_knowledge_space("面试", "D:\\知识库\\面试", PermissionMode::Approval)
            .expect("space is inserted");

        let duplicate =
            store.create_knowledge_space("面试副本", "d:\\知识库\\面试", PermissionMode::Approval);

        assert!(duplicate.is_err());
        assert_eq!(store.count_knowledge_spaces().unwrap(), 1);
    }

    #[test]
    fn soft_deleted_knowledge_space_allows_recreating_root_path() {
        let store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let deleted_id = store
            .create_knowledge_space("面试", "D:\\知识库\\面试", PermissionMode::Approval)
            .expect("space is inserted");
        store
            .connection
            .execute(
                "UPDATE knowledge_spaces SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2",
                params![TEST_TIME, deleted_id],
            )
            .expect("space is soft deleted");

        let recreated_id = store
            .create_knowledge_space("面试新空间", "d:\\知识库\\面试", PermissionMode::Approval)
            .expect("soft-deleted root path can be reused");

        assert_ne!(deleted_id, recreated_id);
        assert_eq!(store.count_knowledge_spaces().unwrap(), 1);
    }

    #[test]
    fn rejects_case_only_duplicate_file_paths() {
        let store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let space_id = store
            .create_knowledge_space("面试", "D:\\知识库\\文件", PermissionMode::Approval)
            .expect("space is inserted");

        insert_file(&store, "file-1", &space_id, "README.md", "indexed").expect("file is inserted");
        let duplicate = insert_file(&store, "file-2", &space_id, "readme.md", "queued");

        assert!(duplicate.is_err());
    }

    #[test]
    fn soft_deleted_file_allows_reinserting_relative_path() {
        let store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let space_id = store
            .create_knowledge_space("面试", "D:\\知识库\\文件", PermissionMode::Approval)
            .expect("space is inserted");

        insert_file(&store, "file-1", &space_id, "README.md", "indexed").expect("file is inserted");
        store
            .connection
            .execute(
                "UPDATE files SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2",
                params![TEST_TIME, "file-1"],
            )
            .expect("file is soft deleted");

        insert_file(&store, "file-2", &space_id, "readme.md", "queued")
            .expect("soft-deleted relative path can be reused");
    }

    #[test]
    fn scan_results_upsert_changed_files_and_soft_delete_missing_files() {
        let mut store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let space_id = store
            .create_knowledge_space("面试", "D:\\知识库\\扫描", PermissionMode::Approval)
            .expect("space is inserted");

        let first_scan = vec![
            scanned_file("README.md", "md", 10, "hash-a"),
            scanned_file("资料\\Redis.pdf", "pdf", 20, "hash-b"),
        ];
        let first_summary = store
            .apply_scan_results(&space_id, &first_scan)
            .expect("first scan applies");

        assert_eq!(first_summary.added_count, 2);
        assert_eq!(store.list_files(&space_id).unwrap().len(), 2);

        let second_scan = vec![
            scanned_file("README.md", "md", 11, "hash-a2"),
            scanned_file("面试题.xlsx", "xlsx", 30, "hash-c"),
        ];
        let second_summary = store
            .apply_scan_results(&space_id, &second_scan)
            .expect("second scan applies");
        let files = store.list_files(&space_id).expect("files list");

        assert_eq!(second_summary.added_count, 1);
        assert_eq!(second_summary.changed_count, 1);
        assert_eq!(second_summary.deleted_count, 1);
        assert_eq!(files.len(), 2);
        assert!(files
            .iter()
            .any(|file| file.name == "README.md"
                && file.status == crate::models::ParseStatus::Changed));
        assert!(files.iter().all(|file| file.name != "Redis.pdf"));
    }

    #[test]
    fn enqueues_document_parse_jobs_for_scanned_candidates_without_duplicates() {
        let mut store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let space_id = store
            .create_knowledge_space("文档", "D:\\知识库\\文档", PermissionMode::Approval)
            .expect("space is inserted");
        let scanned = vec![
            scanned_file("README.md", "md", 10, "hash-a"),
            scanned_file("资料\\Redis.docx", "docx", 20, "hash-b"),
            scanned_file("截图.png", "png", 30, "hash-image"),
        ];
        store
            .apply_scan_results(&space_id, &scanned)
            .expect("scan applies");

        let inserted = store
            .enqueue_document_parse_jobs(&space_id)
            .expect("document jobs enqueue");
        let inserted_again = store
            .enqueue_document_parse_jobs(&space_id)
            .expect("document jobs dedupe");
        let ocr_inserted = store
            .enqueue_image_ocr_parse_jobs(&space_id)
            .expect("image ocr jobs enqueue");
        let ocr_inserted_again = store
            .enqueue_image_ocr_parse_jobs(&space_id)
            .expect("image ocr jobs dedupe");
        let jobs = store.list_parse_jobs(&space_id).expect("jobs list");
        let spaces = store.list_knowledge_spaces().expect("spaces list");

        assert_eq!(inserted, 2);
        assert_eq!(inserted_again, 0);
        assert_eq!(ocr_inserted, 1);
        assert_eq!(ocr_inserted_again, 0);
        assert_eq!(
            jobs.iter()
                .filter(|job| job.job_type == "document" && job.status == "queued")
                .count(),
            2
        );
        assert!(jobs.iter().any(|job| {
            job.job_type == "ocr" && job.status == "queued" && job.file_name == "截图.png"
        }));
        assert_eq!(spaces[0].document_queue_count, 2);
        assert_eq!(spaces[0].ocr_queue_count, 1);
    }

    #[test]
    fn rejects_null_text_id() {
        let store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let result = store.connection.execute(
            "INSERT INTO knowledge_spaces (
                id, name, root_path, default_permission, created_at, updated_at
            )
            VALUES (NULL, ?1, ?2, ?3, ?4, ?4)",
            params!["空 id", "D:\\知识库\\空", "approval", TEST_TIME],
        );

        assert!(result.is_err());
    }

    #[test]
    fn indexes_chinese_knowledge_blocks_with_stable_fts_rowid() {
        let store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let space_id = store
            .create_knowledge_space("面试", "D:\\知识库\\Redis", PermissionMode::Approval)
            .expect("space is inserted");

        insert_knowledge_block(&store, &space_id).expect("block is inserted");
        let fts_rowid = store
            .connection
            .query_row(
                "SELECT fts_rowid FROM knowledge_blocks WHERE id = ?1",
                ["block-1"],
                |row| row.get::<_, i64>(0),
            )
            .expect("fts_rowid is generated");
        let hits = store
            .connection
            .prepare("SELECT rowid FROM knowledge_blocks_fts WHERE knowledge_blocks_fts MATCH ?1")
            .expect("fts query prepares")
            .query_map(["缓存穿透"], |row| row.get::<_, i64>(0))
            .expect("fts query works")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("fts rows decode");

        assert_eq!(hits, vec![fts_rowid]);
    }

    #[test]
    fn stores_parsed_document_as_searchable_knowledge_block() {
        let mut store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let space_id = store
            .create_knowledge_space("面试", "D:\\知识库\\Redis", PermissionMode::Approval)
            .expect("space is inserted");
        insert_file(&store, "file-redis", &space_id, "Redis面试.md", "queued")
            .expect("file is inserted");
        let document = ParsedDocument {
            title: "Redis面试.md".to_string(),
            body: "Redis 缓存穿透是查询不存在的数据导致缓存和数据库都无法命中。".to_string(),
            summary: "Redis 缓存穿透是查询不存在数据导致的缓存失效问题。".to_string(),
            source_locator: "Redis面试.md".to_string(),
            segments: Vec::new(),
            table_insights: Vec::new(),
        };

        store
            .replace_file_knowledge_block(&space_id, "file-redis", &document)
            .expect("knowledge block is stored");

        let hits = store
            .search_knowledge_blocks(&space_id, "缓存穿透", 3)
            .expect("search succeeds");
        let files = store.list_files(&space_id).expect("files list");

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source_file_name, "Redis面试.md");
        assert_eq!(hits[0].source_kind, "original_file");
        assert!(hits[0].excerpt.contains("缓存穿透"));
        assert_eq!(files[0].status, crate::models::ParseStatus::Indexed);
    }

    #[test]
    fn stores_document_segments_as_page_level_blocks() {
        let mut store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let space_id = store
            .create_knowledge_space("PDF", "D:\\知识库\\PDF", PermissionMode::Approval)
            .expect("space is inserted");
        insert_file(&store, "file-pdf", &space_id, "report.pdf", "queued")
            .expect("file is inserted");
        let document = ParsedDocument {
            title: "report.pdf".to_string(),
            body: "第一页介绍总览 第二页包含发票金额".to_string(),
            summary: "PDF 页级证据".to_string(),
            source_locator: "report.pdf".to_string(),
            segments: vec![
                ParsedDocumentSegment {
                    title: "report.pdf · 第 1 页".to_string(),
                    body: "第一页介绍总览".to_string(),
                    source_locator: "report.pdf#page-001".to_string(),
                    evidence: None,
                },
                ParsedDocumentSegment {
                    title: "report.pdf · 第 2 页".to_string(),
                    body: "第二页包含发票金额".to_string(),
                    source_locator: "report.pdf#page-002".to_string(),
                    evidence: Some(ParsedEvidenceMetadata {
                        kind: Some("pdf_page".to_string()),
                        page_number: Some(2),
                        page_count: Some(3),
                        image_number: None,
                        line_count: Some(1),
                        char_count: Some(9),
                        confidence_percent: None,
                    }),
                },
            ],
            table_insights: Vec::new(),
        };

        store
            .replace_file_knowledge_block(&space_id, "file-pdf", &document)
            .expect("knowledge blocks are stored");

        let hits = store
            .search_knowledge_blocks(&space_id, "发票金额", 3)
            .expect("search succeeds");
        let context = store
            .knowledge_block_context(&space_id, &hits[0].id)
            .expect("context query succeeds")
            .expect("context exists");

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source_locator, "report.pdf#page-002");
        assert_eq!(hits[0].source_file_name, "report.pdf");
        assert!(hits[0].excerpt.contains("PDF 第 2/3 页"));
        assert!(hits[0].excerpt.contains("9 字"));
        assert_eq!(context.total_count, 2);
        assert_eq!(context.blocks[0].source_locator, "report.pdf#page-001");
        assert_eq!(context.blocks[1].source_locator, "report.pdf#page-002");
        assert!(context.blocks[1].excerpt.contains("证据范围"));
    }

    #[test]
    fn stores_docx_embedded_image_segments_as_searchable_evidence() {
        let mut store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let space_id = store
            .create_knowledge_space("DOCX", "D:\\知识库\\DOCX", PermissionMode::Approval)
            .expect("space is inserted");
        insert_file(&store, "file-docx", &space_id, "架构说明.docx", "queued")
            .expect("file is inserted");
        let document = ParsedDocument {
            title: "架构说明.docx".to_string(),
            body: "正文和文档图片占位".to_string(),
            summary: "DOCX 图片证据".to_string(),
            source_locator: "架构说明.docx".to_string(),
            segments: vec![
                ParsedDocumentSegment {
                    title: "架构说明.docx".to_string(),
                    body: "正文介绍系统结构。".to_string(),
                    source_locator: "架构说明.docx".to_string(),
                    evidence: None,
                },
                ParsedDocumentSegment {
                    title: "架构说明.docx · 文档图片 1".to_string(),
                    body: "当前仅登记文档内图片和可用替代文本。替代文本：系统架构图".to_string(),
                    source_locator: "架构说明.docx#image-001".to_string(),
                    evidence: Some(ParsedEvidenceMetadata {
                        kind: Some("embedded_image".to_string()),
                        page_number: None,
                        page_count: None,
                        image_number: Some(1),
                        line_count: Some(5),
                        char_count: Some(110),
                        confidence_percent: None,
                    }),
                },
            ],
            table_insights: Vec::new(),
        };

        store
            .replace_file_knowledge_block(&space_id, "file-docx", &document)
            .expect("knowledge blocks are stored");

        let hits = store
            .search_knowledge_blocks(&space_id, "系统架构图", 3)
            .expect("search succeeds");

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source_locator, "架构说明.docx#image-001");
        assert_eq!(hits[0].source_file_name, "架构说明.docx");
        assert!(hits[0].excerpt.contains("文档图片 1"));
        assert!(hits[0].excerpt.contains("110 字"));
    }

    #[test]
    fn document_parse_enqueues_docx_embedded_image_ocr_without_replacing_original_blocks() {
        let mut store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let space_id = store
            .create_knowledge_space("DOCX", "D:\\知识库\\DOCX", PermissionMode::Approval)
            .expect("space is inserted");
        insert_file(&store, "file-docx", &space_id, "架构说明.docx", "queued")
            .expect("file is inserted");
        let job_id = store
            .enqueue_parse_job(&space_id, "file-docx", "document")
            .expect("document job enqueued");
        store.mark_parse_job_running(&job_id).expect("job starts");

        let document = ParsedDocument {
            title: "架构说明.docx".to_string(),
            body: "图片前的正文\n系统架构图".to_string(),
            summary: "DOCX 图片证据".to_string(),
            source_locator: "架构说明.docx".to_string(),
            segments: vec![
                ParsedDocumentSegment {
                    title: "架构说明.docx".to_string(),
                    body: "图片前的正文".to_string(),
                    source_locator: "架构说明.docx".to_string(),
                    evidence: None,
                },
                ParsedDocumentSegment {
                    title: "架构说明.docx · 文档图片 1".to_string(),
                    body: "架构说明.docx · 文档图片 1\n替代文本：系统架构图".to_string(),
                    source_locator: "架构说明.docx#image-001".to_string(),
                    evidence: Some(ParsedEvidenceMetadata {
                        kind: Some("embedded_image".to_string()),
                        page_number: None,
                        page_count: None,
                        image_number: Some(1),
                        line_count: Some(2),
                        char_count: Some(35),
                        confidence_percent: None,
                    }),
                },
            ],
            table_insights: Vec::new(),
        };

        let outcome = store
            .complete_parse_job_if_running(&space_id, "file-docx", &job_id, &document)
            .expect("document parse completes");
        let jobs = store.list_parse_jobs(&space_id).expect("jobs list");
        let hits = store
            .search_knowledge_blocks(&space_id, "图片前的正文", 3)
            .expect("original text is searchable");

        assert_eq!(outcome, ParseJobWriteOutcome::Updated);
        assert!(hits.iter().any(|hit| hit.source_locator == "架构说明.docx"));
        assert!(jobs.iter().any(|job| {
            job.job_type == "ocr"
                && job.status == "queued"
                && job.file_name == "架构说明.docx · 文档图片 1"
                && job.source_locator.as_deref() == Some("架构说明.docx#image-001")
        }));
    }

    #[test]
    fn splits_long_page_segment_without_losing_page_locator() {
        let mut store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let space_id = store
            .create_knowledge_space("OCR", "D:\\知识库\\OCR", PermissionMode::Approval)
            .expect("space is inserted");
        insert_file(&store, "file-scan", &space_id, "scan.pdf", "queued")
            .expect("file is inserted");
        let document = ParsedDocument {
            title: "scan.pdf".to_string(),
            body: "OCR 总文本".to_string(),
            summary: "OCR 页级证据".to_string(),
            source_locator: "scan.pdf#ocr".to_string(),
            segments: vec![ParsedDocumentSegment {
                title: "scan.pdf · OCR 第 1 页".to_string(),
                body: "发票金额".repeat(700),
                source_locator: "scan.pdf#ocr-page-001".to_string(),
                evidence: None,
            }],
            table_insights: Vec::new(),
        };

        store
            .replace_file_knowledge_block(&space_id, "file-scan", &document)
            .expect("knowledge blocks are stored");

        let hits = store
            .search_knowledge_blocks(&space_id, "发票金额", 5)
            .expect("search succeeds");

        assert!(hits.len() > 1);
        assert_eq!(hits[0].source_kind, "ocr");
        assert_eq!(hits[0].source_locator, "scan.pdf#ocr-page-001#block-001");
        assert_eq!(hits[1].source_locator, "scan.pdf#ocr-page-001#block-002");
    }

    #[test]
    fn exports_space_backup_with_metadata_blocks_jobs_and_workspace_settings() {
        let mut store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let space_id = store
            .create_knowledge_space("面试", "D:\\知识库\\Redis", PermissionMode::Approval)
            .expect("space is inserted");
        insert_file(&store, "file-redis", &space_id, "Redis面试.md", "queued")
            .expect("file is inserted");
        let document = ParsedDocument {
            title: "Redis面试.md".to_string(),
            body: "缓存穿透需要空值缓存和布隆过滤器。".to_string(),
            summary: "Redis 缓存穿透资料。".to_string(),
            source_locator: "Redis面试.md".to_string(),
            segments: Vec::new(),
            table_insights: Vec::new(),
        };
        store
            .replace_file_knowledge_block(&space_id, "file-redis", &document)
            .expect("knowledge block is stored");
        let job_id = store
            .enqueue_parse_job(&space_id, "file-redis", "document")
            .expect("parse job is inserted");
        store
            .mark_parse_job_running(&job_id)
            .expect("parse job is running");
        store
            .mark_parse_job_failed(
                &job_id,
                "OCR_RUNTIME_ERROR：DEEPSEEK_API_KEY .env E:\\CodeHome\\Library\\models\\ocr\\pp-ocrv6\\library-ocr-runs\\page.png",
            )
            .expect("parse job failure is recorded");
        store
            .connection
            .execute(
                "INSERT INTO markdown_notes (
                    id, file_id, space_id, relative_path, user_editable, created_at, updated_at
                 )
                 VALUES (?1, ?2, ?3, ?4, 1, ?5, ?5)",
                params![
                    "note-safe",
                    "file-redis",
                    space_id,
                    "Redis面试.note.md",
                    TEST_TIME
                ],
            )
            .expect("safe note is inserted");
        store
            .connection
            .execute(
                "INSERT INTO trash_entries (
                    id, space_id, entity_kind, entity_id, display_name, original_locator, deleted_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    "trash-safe",
                    space_id,
                    "file",
                    "file-redis",
                    "Redis面试.md",
                    "Redis面试.md",
                    TEST_TIME
                ],
            )
            .expect("safe trash entry is inserted");
        insert_file(&store, "file-env", &space_id, ".env", "queued")
            .expect("sensitive file is inserted");
        store
            .connection
            .execute(
                "INSERT INTO markdown_notes (
                    id, file_id, space_id, relative_path, user_editable, created_at, updated_at
                 )
                 VALUES (?1, ?2, ?3, ?4, 1, ?5, ?5)",
                params![
                    "note-sensitive",
                    "file-env",
                    space_id,
                    "models\\ocr\\pp-ocrv6\\secret-note.md",
                    TEST_TIME
                ],
            )
            .expect("sensitive note is inserted");
        store
            .connection
            .execute(
                "INSERT INTO knowledge_blocks (
                    id, space_id, file_id, title, body, source_kind, source_locator, created_at, updated_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                params![
                    "block-sensitive",
                    space_id,
                    "file-env",
                    "敏感配置",
                    "should-not-export",
                    "original_file",
                    ".env#block-001",
                    TEST_TIME
                ],
            )
            .expect("sensitive block is inserted");
        store
            .connection
            .execute(
                "INSERT INTO trash_entries (
                    id, space_id, entity_kind, entity_id, display_name, original_locator, deleted_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    "trash-sensitive",
                    space_id,
                    "file",
                    "file-env",
                    "page.png",
                    "library-ocr-runs\\page.png",
                    TEST_TIME
                ],
            )
            .expect("sensitive trash entry is inserted");

        let backup = store
            .export_space_backup(&space_id)
            .expect("backup export is built");
        let serialized = serde_json::to_string(&backup).expect("backup serializes");

        assert_eq!(backup.format, "library.backup.v1");
        assert_eq!(backup.space.id, space_id);
        assert_eq!(backup.space.name, "面试");
        assert_eq!(
            backup.workspace.default_permission,
            PermissionMode::Approval
        );
        assert_eq!(backup.files.len(), 1);
        assert_eq!(backup.files[0].relative_path, "Redis面试.md");
        assert_eq!(backup.markdown_notes.len(), 1);
        assert_eq!(backup.markdown_notes[0].relative_path, "Redis面试.note.md");
        assert_eq!(backup.knowledge_blocks.len(), 1);
        assert!(backup.knowledge_blocks[0].body.contains("布隆过滤器"));
        assert_eq!(backup.parse_jobs.len(), 1);
        assert_eq!(backup.parse_jobs[0].job_type, "document");
        assert_eq!(backup.parse_jobs[0].status, "failed");
        assert!(backup.parse_jobs[0].error_message.is_none());
        assert_eq!(backup.trash_entries.len(), 1);
        assert_eq!(backup.trash_entries[0].original_locator, "Redis面试.md");
        assert!(!serialized.contains("DEEPSEEK_API_KEY"));
        assert!(!serialized.contains(".env"));
        assert!(!serialized.contains("models"));
        assert!(!serialized.contains("secret-note"));
        assert!(!serialized.contains("library-ocr-runs"));
        assert!(!serialized.contains("should-not-export"));
    }

    #[test]
    fn restores_space_backup_replacing_existing_space_and_search_index() {
        let mut store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        store
            .connection
            .execute(
                "INSERT INTO knowledge_spaces (
                    id, name, root_path, default_permission, created_at, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![
                    "backup-space",
                    "旧空间",
                    "D:\\知识库\\旧",
                    "readonly",
                    TEST_TIME
                ],
            )
            .expect("existing space is inserted");
        insert_file(&store, "old-file", "backup-space", "旧文件.md", "indexed")
            .expect("old file is inserted");
        insert_knowledge_block(&store, "backup-space").expect("old block is inserted");
        let backup = backup_export_fixture();

        store
            .restore_space_backup(&backup)
            .expect("backup is restored");
        let spaces = store.list_knowledge_spaces().expect("spaces list");
        let files = store.list_files("backup-space").expect("files list");
        let hits = store
            .search_knowledge_blocks("backup-space", "空值缓存", 5)
            .expect("search restored blocks");
        let old_hits = store
            .search_knowledge_blocks("backup-space", "缓存雪崩", 5)
            .expect("search old blocks");

        assert_eq!(spaces.len(), 1);
        assert_eq!(spaces[0].name, "备份空间");
        assert_eq!(spaces[0].default_permission, PermissionMode::Approval);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "Redis面试.md");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source_locator, "Redis面试.md");
        assert!(old_hits.is_empty());
    }

    #[test]
    fn stores_table_insights_as_searchable_table_blocks() {
        let mut store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let space_id = store
            .create_knowledge_space("报表", "D:\\知识库\\报表", PermissionMode::Approval)
            .expect("space is inserted");
        insert_file(&store, "file-report", &space_id, "经营报表.xlsx", "queued")
            .expect("file is inserted");
        let document = ParsedDocument {
            title: "经营报表.xlsx".to_string(),
            body: "工作簿普通文本".to_string(),
            summary: "工作簿普通文本".to_string(),
            source_locator: "经营报表.xlsx".to_string(),
            segments: Vec::new(),
            table_insights: vec![ParsedTableInsight {
                title: "经营报表.xlsx · 工作表 1".to_string(),
                body: "经营报表.xlsx · 工作表 1 结构：3 行，3 列 表头：月份、营收、成本 样例 1：2026-06 | 120 | 70 可问答字段：月份、营收、成本".to_string(),
                summary: "工作表 1：3 行、3 列；表头：月份、营收、成本".to_string(),
                source_locator: "经营报表.xlsx#sheet-001".to_string(),
            }],
        };

        store
            .replace_file_knowledge_block(&space_id, "file-report", &document)
            .expect("knowledge blocks are stored");

        let table_count: i64 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM knowledge_blocks WHERE file_id = ?1 AND source_kind = 'table' AND searchable = 1",
                ["file-report"],
                |row| row.get(0),
            )
            .expect("table block count query works");
        let preview = store
            .latest_table_insight(&space_id)
            .expect("latest table query succeeds")
            .expect("table preview exists");
        let hits = store
            .search_knowledge_blocks(&space_id, "营收", 3)
            .expect("search succeeds");

        assert_eq!(table_count, 1);
        assert_eq!(preview.title, "经营报表.xlsx · 工作表 1");
        assert!(preview.description.contains("3 行"));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source_locator, "经营报表.xlsx#sheet-001");
        assert_eq!(hits[0].source_file_name, "经营报表.xlsx");
        assert_eq!(hits[0].source_kind, "table");
    }

    #[test]
    fn search_ranks_table_sources_before_plain_file_for_table_queries() {
        let store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let space_id = store
            .create_knowledge_space("报表", "D:\\知识库\\报表排序", PermissionMode::Approval)
            .expect("space is inserted");
        insert_file(&store, "file-report", &space_id, "经营报表.xlsx", "indexed")
            .expect("file is inserted");
        store
            .connection
            .execute(
                "INSERT INTO knowledge_blocks (
                    id, space_id, file_id, title, body, source_kind, source_locator, created_at, updated_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                params![
                    "block-plain-report",
                    space_id,
                    "file-report",
                    "2026-06 营收说明",
                    "2026-06 营收在正文里被提到，但没有表头和样例行。",
                    "original_file",
                    "经营报表.xlsx",
                    TEST_TIME
                ],
            )
            .expect("plain block is inserted");
        store
            .connection
            .execute(
                "INSERT INTO knowledge_blocks (
                    id, space_id, file_id, title, body, source_kind, source_locator, created_at, updated_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                params![
                    "block-table-report",
                    space_id,
                    "file-report",
                    "经营报表.xlsx · 工作表 1",
                    "表头：月份、营收、成本。样例 1：2026-06 | 120 | 70。",
                    "table",
                    "经营报表.xlsx#sheet-001",
                    TEST_TIME
                ],
            )
            .expect("table block is inserted");

        let hits = store
            .search_knowledge_blocks(&space_id, "2026-06 营收", 4)
            .expect("search succeeds");

        assert!(hits.len() >= 2);
        assert_eq!(hits[0].source_kind, "table");
        assert_eq!(hits[0].source_locator, "经营报表.xlsx#sheet-001");
    }

    #[test]
    fn source_ranking_promotes_adjacent_structured_sources_only() {
        let mut hits = vec![
            ranked_hit("plain-close", "original_file", "经营报表.md"),
            ranked_hit("table-close", "table", "经营报表.xlsx#sheet-001"),
        ];

        rank_search_hits(&mut hits, "营收");

        assert_eq!(hits[0].id, "table-close");
        assert_eq!(hits[1].id, "plain-close");
    }

    #[test]
    fn source_ranking_keeps_clear_relevance_leaders_first() {
        let mut hits = vec![
            ranked_hit("plain-strong", "original_file", "经营报表.md#block-001"),
            ranked_hit("plain-next", "original_file", "经营报表.md#block-002"),
            ranked_hit("plain-third", "original_file", "经营报表.md#block-003"),
            ranked_hit("table-weak", "table", "历史报表.xlsx#sheet-001"),
        ];

        rank_search_hits(&mut hits, "营收");

        assert_eq!(hits[0].id, "plain-strong");
        assert_eq!(hits[1].id, "plain-next");
        assert_eq!(hits[2].id, "table-weak");
    }

    #[test]
    fn search_ranks_ocr_sources_before_plain_file_for_scan_queries() {
        let store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let space_id = store
            .create_knowledge_space("OCR", "D:\\知识库\\OCR排序", PermissionMode::Approval)
            .expect("space is inserted");
        insert_file(&store, "file-scan", &space_id, "scan.pdf", "indexed")
            .expect("file is inserted");
        store
            .connection
            .execute(
                "INSERT INTO knowledge_blocks (
                    id, space_id, file_id, title, body, source_kind, source_locator, created_at, updated_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                params![
                    "block-plain-scan",
                    space_id,
                    "file-scan",
                    "发票金额说明",
                    "普通摘要里提到扫描版发票金额。",
                    "original_file",
                    "scan.pdf",
                    TEST_TIME
                ],
            )
            .expect("plain block is inserted");
        store
            .connection
            .execute(
                "INSERT INTO knowledge_blocks (
                    id, space_id, file_id, title, body, source_kind, source_locator, created_at, updated_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                params![
                    "block-ocr-scan",
                    space_id,
                    "file-scan",
                    "scan.pdf · OCR 片段 1/1",
                    "本地 OCR 识别到扫描版发票金额为 120 元。",
                    "original_file",
                    "scan.pdf#ocr-block-001",
                    TEST_TIME
                ],
            )
            .expect("ocr block is inserted");

        let hits = store
            .search_knowledge_blocks(&space_id, "扫描版 发票金额", 4)
            .expect("search succeeds");

        assert!(hits.len() >= 2);
        assert_eq!(hits[0].source_kind, "ocr");
        assert_eq!(hits[0].source_locator, "scan.pdf#ocr-block-001");
    }

    #[test]
    fn stores_long_parsed_document_as_chunked_searchable_blocks() {
        let mut store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let space_id = store
            .create_knowledge_space("面试", "D:\\知识库\\Redis", PermissionMode::Approval)
            .expect("space is inserted");
        insert_file(&store, "file-redis", &space_id, "Redis面试.md", "queued")
            .expect("file is inserted");
        let document = ParsedDocument {
            title: "Redis面试.md".to_string(),
            body: format!(
                "{}。{}",
                "缓存预热和键空间检查".repeat(130),
                "布隆过滤器可以拦截不存在的键".repeat(100)
            ),
            summary: "Redis 缓存穿透资料。".to_string(),
            source_locator: "Redis面试.md".to_string(),
            segments: Vec::new(),
            table_insights: Vec::new(),
        };

        store
            .replace_file_knowledge_block(&space_id, "file-redis", &document)
            .expect("knowledge blocks are stored");

        let block_count: i64 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM knowledge_blocks WHERE file_id = ?1 AND searchable = 1",
                ["file-redis"],
                |row| row.get(0),
            )
            .expect("block count query works");
        let hits = store
            .search_knowledge_blocks(&space_id, "布隆过滤器", 3)
            .expect("search succeeds");
        let context = store
            .knowledge_block_context(&space_id, &hits[0].id)
            .expect("context query succeeds")
            .expect("context exists");

        assert_eq!(block_count, 2);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source_locator, "Redis面试.md#block-002");
        assert_eq!(hits[0].source_file_name, "Redis面试.md");
        assert!(hits[0].title.contains("片段 2/2"));
        assert!(hits[0].excerpt.contains("布隆过滤器"));
        assert_eq!(context.current_index, 2);
        assert_eq!(context.total_count, 2);
        assert_eq!(context.blocks[0].source_locator, "Redis面试.md#block-001");
        assert_eq!(context.blocks[1].source_locator, "Redis面试.md#block-002");
    }

    #[test]
    fn stores_long_ocr_document_with_ocr_chunk_locators() {
        let mut store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let space_id = store
            .create_knowledge_space("OCR", "D:\\知识库\\OCR", PermissionMode::Approval)
            .expect("space is inserted");
        insert_file(&store, "file-scan", &space_id, "scan.pdf", "queued")
            .expect("file is inserted");
        let document = ParsedDocument {
            title: "scan.pdf".to_string(),
            body: format!(
                "{}。{}",
                "扫描版第一页内容".repeat(160),
                "扫描版第二页包含发票金额".repeat(120)
            ),
            summary: "扫描版 PDF OCR 结果。".to_string(),
            source_locator: "scan.pdf#ocr".to_string(),
            segments: Vec::new(),
            table_insights: Vec::new(),
        };

        store
            .replace_file_knowledge_block(&space_id, "file-scan", &document)
            .expect("knowledge blocks are stored");

        let hits = store
            .search_knowledge_blocks(&space_id, "发票金额", 3)
            .expect("search succeeds");

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source_locator, "scan.pdf#ocr-block-002");
        assert_eq!(hits[0].source_file_name, "scan.pdf");
        assert!(hits[0].title.contains("片段 2/2"));
        assert!(hits[0].excerpt.contains("发票金额"));
    }

    #[test]
    fn source_file_name_hides_ocr_locator_suffix() {
        assert_eq!(
            display_source_file_name("扫描资料\\scan.pdf#ocr"),
            "scan.pdf"
        );
        assert_eq!(
            display_source_file_name("扫描资料\\scan.pdf#ocr-block-002"),
            "scan.pdf"
        );
        assert_eq!(
            display_source_file_name("扫描资料\\scan.pdf#ocr-page-002"),
            "scan.pdf"
        );
        assert_eq!(
            display_source_file_name("扫描资料\\report.pdf#page-003"),
            "report.pdf"
        );
        assert_eq!(
            display_source_file_name("docs\\Redis面试.md#block-001"),
            "Redis面试.md"
        );
        assert_eq!(
            display_source_file_name("reports\\经营报表.xlsx#sheet-001"),
            "经营报表.xlsx"
        );
        assert_eq!(
            display_source_file_name("docs\\架构说明.docx#image-001"),
            "架构说明.docx"
        );
        assert_eq!(display_source_file_name("Redis面试.md"), "Redis面试.md");
    }

    #[test]
    fn enqueues_and_cancels_parse_job() {
        let store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let space_id = store
            .create_knowledge_space("OCR", "D:\\知识库\\OCR", PermissionMode::Approval)
            .expect("space is inserted");
        insert_file(&store, "file-scan", &space_id, "scan.pdf", "queued")
            .expect("file is inserted");

        let job_id = store
            .enqueue_parse_job(&space_id, "file-scan", "ocr")
            .expect("job enqueued");
        let duplicate_job_id = store
            .enqueue_parse_job(&space_id, "file-scan", "ocr")
            .expect("existing active job reused");
        let cancelled = store.cancel_parse_job(&job_id).expect("job cancelled");
        let retried_job_id = store
            .enqueue_parse_job(&space_id, "file-scan", "ocr")
            .expect("cancelled job can be retried");
        let jobs = store.list_parse_jobs(&space_id).expect("jobs list");

        assert_eq!(duplicate_job_id, job_id);
        assert!(cancelled);
        assert_ne!(retried_job_id, job_id);
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].file_id.as_deref(), Some("file-scan"));
        assert_eq!(jobs[0].status, "queued");
        assert_eq!(jobs[0].phase, "等待执行");
        assert_eq!(jobs[0].progress_current, 0);
        assert_eq!(jobs[0].progress_total, 1);
        assert_eq!(jobs[1].status, "cancelled");
        assert_eq!(jobs[1].phase, "已取消");
        assert!(jobs[1].finished_at.is_some());
    }

    #[test]
    fn moves_queued_parse_job_through_running_and_succeeded_states() {
        let store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let space_id = store
            .create_knowledge_space("OCR", "D:\\知识库\\OCR", PermissionMode::Approval)
            .expect("space is inserted");
        insert_file(&store, "file-scan", &space_id, "scan.pdf", "queued")
            .expect("file is inserted");
        let job_id = store
            .enqueue_parse_job(&space_id, "file-scan", "ocr")
            .expect("job enqueued");

        let candidate = store
            .next_queued_parse_job(&space_id, "ocr")
            .expect("query succeeds")
            .expect("queued job exists");
        let started = store
            .mark_parse_job_running(&candidate.job_id)
            .expect("job starts");
        let progressed = store
            .update_parse_job_progress(&candidate.job_id, "正在执行本地 OCR", 0, 1)
            .expect("job progress updates");
        let succeeded = store
            .mark_parse_job_succeeded(&candidate.job_id)
            .expect("job succeeds");
        let jobs = store.list_parse_jobs(&space_id).expect("jobs list");

        assert_eq!(candidate.job_id, job_id);
        assert_eq!(candidate.file_id, "file-scan");
        assert_eq!(candidate.relative_path, "scan.pdf");
        assert!(started);
        assert!(progressed);
        assert!(succeeded);
        assert_eq!(jobs[0].status, "succeeded");
        assert!(jobs[0].error_message.is_none());
        assert!(jobs[0].started_at.is_some());
        assert!(jobs[0].finished_at.is_some());
        assert_eq!(jobs[0].progress_current, 1);
        assert_eq!(jobs[0].progress_total, 1);
        assert_eq!(jobs[0].phase, "已完成");
    }

    #[test]
    fn records_failed_parse_job_message() {
        let store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let space_id = store
            .create_knowledge_space("OCR", "D:\\知识库\\OCR", PermissionMode::Approval)
            .expect("space is inserted");
        insert_file(&store, "file-scan", &space_id, "scan.pdf", "queued")
            .expect("file is inserted");
        let job_id = store
            .enqueue_parse_job(&space_id, "file-scan", "ocr")
            .expect("job enqueued");

        store.mark_parse_job_running(&job_id).expect("job starts");
        let failed = store
            .mark_parse_job_failed(&job_id, "OCR_EMPTY_RESULT")
            .expect("job fails");
        let jobs = store.list_parse_jobs(&space_id).expect("jobs list");

        assert!(failed);
        assert_eq!(jobs[0].status, "failed");
        assert_eq!(jobs[0].error_message.as_deref(), Some("OCR_EMPTY_RESULT"));
        assert!(jobs[0].finished_at.is_some());
        assert_eq!(jobs[0].phase, "失败");
    }

    #[test]
    fn cancels_running_parse_job_without_allowing_success_transition() {
        let store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let space_id = store
            .create_knowledge_space("OCR", "D:\\知识库\\OCR", PermissionMode::Approval)
            .expect("space is inserted");
        insert_file(&store, "file-scan", &space_id, "scan.pdf", "queued")
            .expect("file is inserted");
        let job_id = store
            .enqueue_parse_job(&space_id, "file-scan", "ocr")
            .expect("job enqueued");

        store.mark_parse_job_running(&job_id).expect("job starts");
        store
            .update_parse_job_progress(&job_id, "正在执行本地 OCR", 0, 1)
            .expect("progress updates");
        let cancelled = store.cancel_parse_job(&job_id).expect("job cancels");
        let succeeded = store
            .mark_parse_job_succeeded(&job_id)
            .expect("cancelled job cannot succeed");
        let jobs = store.list_parse_jobs(&space_id).expect("jobs list");

        assert!(cancelled);
        assert!(!succeeded);
        assert_eq!(jobs[0].status, "cancelled");
        assert_eq!(jobs[0].phase, "已取消");
        assert_eq!(
            store
                .parse_job_status(&job_id)
                .expect("status reads")
                .as_deref(),
            Some("cancelled")
        );
    }

    #[test]
    fn documents_trigram_short_chinese_query_behavior() {
        let store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let space_id = store
            .create_knowledge_space("面试", "D:\\知识库\\短词", PermissionMode::Approval)
            .expect("space is inserted");
        insert_knowledge_block(&store, &space_id).expect("block is inserted");

        // FTS5 trigram only indexes three-character-or-longer tokens; a future
        // search API should add a short-query fallback instead of changing this
        // metadata repository into a search service.
        let short_query_hits = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM knowledge_blocks_fts WHERE knowledge_blocks_fts MATCH ?1",
                ["缓存"],
                |row| row.get::<_, u32>(0),
            )
            .expect("fts query works");

        assert_eq!(short_query_hits, 0);
    }

    #[test]
    fn rebuilds_legacy_foundation_schema_without_losing_metadata() {
        let connection = Connection::open_in_memory().expect("in-memory sqlite opens");
        connection
            .execute_batch(LEGACY_FOUNDATION_SCHEMA)
            .expect("legacy schema applies");
        connection
            .execute(
                "INSERT INTO knowledge_spaces (
                    id, name, root_path, default_permission, created_at, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![
                    "legacy-space",
                    "旧空间",
                    "D:\\知识库\\旧",
                    "approval",
                    TEST_TIME
                ],
            )
            .expect("legacy space is inserted");
        insert_legacy_file(&connection).expect("legacy file is inserted");
        connection
            .execute(
                "INSERT INTO knowledge_blocks (
                    id, space_id, file_id, title, body, source_kind, source_locator,
                    searchable, created_at, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
                params![
                    "legacy-block",
                    "legacy-space",
                    "legacy-file",
                    "Redis 缓存",
                    "缓存穿透和缓存雪崩",
                    "original_file",
                    "redis.md#1",
                    1,
                    TEST_TIME
                ],
            )
            .expect("legacy block is inserted");

        let mut store = SqliteStore { connection };
        store
            .apply_foundation_schema()
            .expect("legacy schema is rebuilt");

        store
            .connection
            .execute(
                "UPDATE knowledge_spaces SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2",
                params![TEST_TIME, "legacy-space"],
            )
            .expect("legacy space is soft deleted");
        store
            .create_knowledge_space("新空间", "d:\\知识库\\旧", PermissionMode::Approval)
            .expect("rebuilt schema allows reused root path");
        store
            .connection
            .execute(
                "UPDATE files SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2",
                params![TEST_TIME, "legacy-file"],
            )
            .expect("legacy file is soft deleted");
        insert_file(&store, "new-file", "legacy-space", "readme.md", "queued")
            .expect("rebuilt schema allows reused file path");

        let fts_rowid = store
            .connection
            .query_row(
                "SELECT fts_rowid FROM knowledge_blocks WHERE id = ?1",
                ["legacy-block"],
                |row| row.get::<_, i64>(0),
            )
            .expect("stable fts rowid exists after rebuild");
        let fts_match = store
            .connection
            .query_row(
                "SELECT rowid FROM knowledge_blocks_fts WHERE knowledge_blocks_fts MATCH ?1",
                ["缓存穿透"],
                |row| row.get::<_, i64>(0),
            )
            .expect("fts is rebuilt for legacy block");

        assert_eq!(fts_match, fts_rowid);
    }

    #[test]
    fn upgrades_legacy_parse_jobs_without_losing_queue_state() {
        let connection = Connection::open_in_memory().expect("in-memory sqlite opens");
        connection
            .execute_batch(
                r#"
                CREATE TABLE parse_jobs (
                  id TEXT NOT NULL PRIMARY KEY,
                  space_id TEXT NOT NULL,
                  file_id TEXT,
                  job_type TEXT NOT NULL,
                  status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
                  error_message TEXT,
                  created_at TEXT NOT NULL,
                  updated_at TEXT NOT NULL
                );

                INSERT INTO parse_jobs (
                  id, space_id, file_id, job_type, status, error_message, created_at, updated_at
                )
                VALUES ('legacy-job', 'legacy-space', NULL, 'ocr', 'queued', NULL, '2026-06-21T00:00:00Z', '2026-06-21T00:00:00Z');
                "#,
            )
            .expect("legacy parse job schema applies");

        let mut store = SqliteStore { connection };
        store
            .apply_foundation_schema()
            .expect("legacy parse_jobs schema is upgraded");
        let jobs = store
            .list_parse_jobs("legacy-space")
            .expect("legacy jobs list");

        assert!(store
            .column_exists("parse_jobs", "started_at")
            .expect("started_at column check"));
        assert!(store
            .column_exists("parse_jobs", "phase")
            .expect("phase column check"));
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id, "legacy-job");
        assert_eq!(jobs[0].status, "queued");
        assert_eq!(jobs[0].phase, "等待执行");
        assert_eq!(jobs[0].progress_current, 0);
        assert_eq!(jobs[0].progress_total, 0);
    }

    fn insert_file(
        store: &SqliteStore,
        file_id: &str,
        space_id: &str,
        relative_path: &str,
        parse_status: &str,
    ) -> rusqlite::Result<usize> {
        store.connection.execute(
            "INSERT INTO files (
                id, space_id, relative_path, extension, parse_status, created_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![
                file_id,
                space_id,
                relative_path,
                "md",
                parse_status,
                TEST_TIME
            ],
        )
    }

    fn insert_knowledge_block(store: &SqliteStore, space_id: &str) -> rusqlite::Result<usize> {
        store.connection.execute(
            "INSERT INTO knowledge_blocks (
                id, space_id, title, body, source_kind, source_locator, created_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![
                "block-1",
                space_id,
                "Redis 缓存",
                "缓存穿透和缓存雪崩",
                "original_file",
                "redis.md#1",
                TEST_TIME
            ],
        )
    }

    fn insert_legacy_file(connection: &Connection) -> rusqlite::Result<usize> {
        connection.execute(
            "INSERT INTO files (
                id, space_id, relative_path, extension, parse_status, created_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![
                "legacy-file",
                "legacy-space",
                "README.md",
                "md",
                "indexed",
                TEST_TIME
            ],
        )
    }

    fn backup_export_fixture() -> BackupExport {
        BackupExport {
            format: "library.backup.v1".to_string(),
            schema_version: 1,
            exported_at: TEST_TIME.to_string(),
            space: BackupExportSpace {
                id: "backup-space".to_string(),
                name: "备份空间".to_string(),
                root_path: "D:\\知识库\\备份空间".to_string(),
                default_permission: PermissionMode::Approval,
                created_at: TEST_TIME.to_string(),
                updated_at: TEST_TIME.to_string(),
            },
            workspace: BackupExportWorkspace {
                active_space_id: "backup-space".to_string(),
                default_permission: PermissionMode::Approval,
            },
            files: vec![BackupExportFile {
                id: "file-redis".to_string(),
                relative_path: "Redis面试.md".to_string(),
                extension: "md".to_string(),
                content_hash: Some("hash-redis".to_string()),
                size_bytes: 128,
                modified_at: Some(TEST_TIME.to_string()),
                parse_status: "indexed".to_string(),
                last_scanned_at: Some(TEST_TIME.to_string()),
                created_at: TEST_TIME.to_string(),
                updated_at: TEST_TIME.to_string(),
                deleted_at: None,
            }],
            markdown_notes: Vec::new(),
            knowledge_blocks: vec![BackupExportKnowledgeBlock {
                id: "block-redis".to_string(),
                file_id: Some("file-redis".to_string()),
                note_id: None,
                title: "Redis 缓存".to_string(),
                body: "缓存穿透需要空值缓存。".to_string(),
                source_kind: "original_file".to_string(),
                source_locator: "Redis面试.md".to_string(),
                searchable: true,
                created_at: TEST_TIME.to_string(),
                updated_at: TEST_TIME.to_string(),
                deleted_at: None,
            }],
            parse_jobs: Vec::new(),
            trash_entries: Vec::new(),
        }
    }

    fn scanned_file(
        relative_path: &str,
        extension: &str,
        size_bytes: i64,
        content_hash: &str,
    ) -> ScannedFile {
        ScannedFile {
            relative_path: relative_path.to_string(),
            extension: extension.to_string(),
            size_bytes,
            modified_at: TEST_TIME.to_string(),
            content_hash: content_hash.to_string(),
        }
    }
}
