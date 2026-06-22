use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::models::{
    FileParseCandidate, KnowledgeBlockSearchHit, KnowledgeFile, KnowledgeSpace, ParseJobCandidate,
    ParseJobSummary, ParseStatus, ParsedDocument, PermissionMode, ScanSummary, ScannedFile,
};

const DOCUMENT_PARSE_EXTENSIONS: [&str; 5] = ["pdf", "docx", "xlsx", "md", "txt"];
const AUTO_OCR_EXTENSIONS: [&str; 7] = ["png", "jpg", "jpeg", "bmp", "tif", "tiff", "webp"];
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
        let mut store = Self { connection };
        store.apply_foundation_schema()?;
        Ok(store)
    }

    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let connection = Connection::open_in_memory()?;
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

        replace_file_knowledge_block_in_tx(&tx, space_id, file_id, document, &now)?;
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
                "SELECT id, title, body, source_locator
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

        let mut hits = Vec::new();
        for term in &terms {
            append_unique_hits(
                &mut hits,
                self.search_knowledge_blocks_fts(space_id, term, limit)?,
                limit,
            );
            if hits.len() >= limit {
                return Ok(hits);
            }
        }

        for term in &terms {
            append_unique_hits(
                &mut hits,
                self.search_knowledge_blocks_like(space_id, term, limit)?,
                limit,
            );
            if hits.len() >= limit {
                break;
            }
        }

        Ok(hits)
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
        if let Some(existing_id) = self
            .connection
            .query_row(
                "SELECT id
                 FROM parse_jobs
                 WHERE space_id = ?1
                   AND file_id = ?2
                   AND job_type = ?3
                   AND status IN ('queued', 'running')
                 ORDER BY created_at DESC
                 LIMIT 1",
                params![space_id, file_id, job_type],
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
             VALUES (?1, ?2, ?3, ?4, 'queued', '等待执行', 0, 1, ?5, ?5)",
            params![id, space_id, file_id, job_type, now],
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
                let relative_path: String = row.get(2)?;
                Ok(ParseJobSummary {
                    id: row.get(0)?,
                    file_id: row.get(1)?,
                    file_name: display_file_name(&relative_path),
                    job_type: row.get(3)?,
                    status: row.get(4)?,
                    error_message: row.get(5)?,
                    started_at: row.get(6)?,
                    finished_at: row.get(7)?,
                    progress_current: row.get(8)?,
                    progress_total: row.get(9)?,
                    phase: row.get(10)?,
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
                "SELECT job.id, file.id, file.relative_path, file.extension
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
                "SELECT job.id, file.id, file.relative_path, file.extension
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
            "SELECT block.id, block.title, block.body, block.source_locator
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
            Ok(mapped) => mapped.collect(),
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
            "SELECT id, title, body, source_locator
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
        if enqueue_file_parse_job_in_tx(tx, space_id, &candidate.file_id, "document", now)? {
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
        if enqueue_file_parse_job_in_tx(tx, space_id, &candidate.file_id, "ocr", now)? {
            inserted_count += 1;
        }
    }

    Ok(inserted_count)
}

fn enqueue_file_parse_job_in_tx(
    connection: &Connection,
    space_id: &str,
    file_id: &str,
    job_type: &str,
    now: &str,
) -> rusqlite::Result<bool> {
    let existing_id = connection
        .query_row(
            "SELECT id
             FROM parse_jobs
             WHERE space_id = ?1
               AND file_id = ?2
               AND job_type = ?3
               AND status IN ('queued', 'running')
             ORDER BY created_at DESC
             LIMIT 1",
            params![space_id, file_id, job_type],
            |row| row.get::<_, String>(0),
        )
        .optional()?;

    if existing_id.is_some() {
        return Ok(false);
    }

    connection.execute(
        "INSERT INTO parse_jobs (
            id, space_id, file_id, job_type, status, phase, progress_current,
            progress_total, created_at, updated_at
         )
         VALUES (?1, ?2, ?3, ?4, 'queued', '等待执行', 0, 1, ?5, ?5)",
        params![Uuid::new_v4().to_string(), space_id, file_id, job_type, now],
    )?;

    Ok(true)
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

    tx.execute(
        "INSERT INTO knowledge_blocks (
            id, space_id, file_id, title, body, source_kind, source_locator,
            searchable, created_at, updated_at
         )
         VALUES (?1, ?2, ?3, ?4, ?5, 'original_file', ?6, 1, ?7, ?7)",
        params![
            Uuid::new_v4().to_string(),
            space_id,
            file_id,
            document.title,
            format!("{}\n\n{}", document.summary, document.body),
            document.source_locator,
            now
        ],
    )?;

    tx.execute(
        "UPDATE files
         SET parse_status = ?1, updated_at = ?2
         WHERE id = ?3 AND space_id = ?4 AND deleted_at IS NULL",
        params![ParseStatus::Indexed.as_str(), now, file_id, space_id],
    )?;

    Ok(())
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
        |row| row.get(0),
    )
    .optional()
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

    Ok(KnowledgeBlockSearchHit {
        id: row.get(0)?,
        title: row.get(1)?,
        excerpt: build_excerpt(&body, term),
        source_file_name: display_source_file_name(&source_locator),
        source_locator,
    })
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
    let normalized = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let excerpt = if !term.is_empty() {
        normalized
            .find(term)
            .map(|index| normalized[index..].to_string())
            .unwrap_or_else(|| normalized.clone())
    } else {
        normalized
    };

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

fn display_source_file_name(source_locator: &str) -> String {
    let source_path = source_locator
        .strip_suffix("#ocr")
        .unwrap_or(source_locator);
    display_file_name(source_path)
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
    use super::{display_source_file_name, SqliteStore};
    use crate::models::{ParsedDocument, PermissionMode, ScannedFile};
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
        assert!(hits[0].excerpt.contains("缓存穿透"));
        assert_eq!(files[0].status, crate::models::ParseStatus::Indexed);
    }

    #[test]
    fn source_file_name_hides_ocr_locator_suffix() {
        assert_eq!(
            display_source_file_name("扫描资料\\scan.pdf#ocr"),
            "scan.pdf"
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
