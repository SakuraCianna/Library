use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::models::{
    KnowledgeFile, KnowledgeSpace, ParseStatus, PermissionMode, ScanSummary, ScannedFile,
};

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
                COALESCE(queued.queued_count, 0) AS queued_count
             FROM knowledge_spaces space
             LEFT JOIN (
                SELECT space_id, COUNT(*) AS changed_count
                FROM files
                WHERE deleted_at IS NULL AND parse_status = 'changed'
                GROUP BY space_id
             ) changed ON changed.space_id = space.id
             LEFT JOIN (
                SELECT space_id, COUNT(*) AS queued_count
                FROM files
                WHERE deleted_at IS NULL AND parse_status = 'queued'
                GROUP BY space_id
             ) queued ON queued.space_id = space.id
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
                    ocr_queue_count: row.get(5)?,
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

    pub fn count_knowledge_spaces(&self) -> rusqlite::Result<u32> {
        self.connection.query_row(
            "SELECT COUNT(*) FROM knowledge_spaces WHERE deleted_at IS NULL",
            [],
            |row| row.get::<_, u32>(0),
        )
    }

    pub fn apply_scan_results(
        &mut self,
        space_id: &str,
        scanned_files: &[ScannedFile],
    ) -> rusqlite::Result<ScanSummary> {
        let now = OffsetDateTime::now_utc().to_string();
        let scan_run_id = Uuid::new_v4().to_string();
        let tx = self.connection.transaction()?;
        tx.execute(
            "INSERT INTO scan_runs (id, space_id, started_at, status)
             VALUES (?1, ?2, ?3, 'running')",
            params![scan_run_id, space_id, now],
        )?;

        let existing_files = load_existing_files(&tx, space_id)?;
        let mut seen_keys = Vec::with_capacity(scanned_files.len());
        let mut summary = ScanSummary::default();

        for scanned_file in scanned_files {
            let key = normalize_lookup_key(&scanned_file.relative_path);
            seen_keys.push(key.clone());

            match existing_files.iter().find(|file| file.lookup_key == key) {
                Some(existing) if existing.deleted_at.is_none() => {
                    let changed = existing.content_hash.as_deref()
                        != Some(scanned_file.content_hash.as_str())
                        || existing.modified_at.as_deref()
                            != Some(scanned_file.modified_at.as_str())
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

fn display_file_name(relative_path: &str) -> String {
    relative_path
        .rsplit(['\\', '/'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(relative_path)
        .to_string()
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
    use super::SqliteStore;
    use crate::models::{PermissionMode, ScannedFile};
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
