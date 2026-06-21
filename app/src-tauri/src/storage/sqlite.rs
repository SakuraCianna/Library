use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

const FOUNDATION_SCHEMA: &str = include_str!("../../migrations/001_foundation.sql");
const FOUNDATION_TABLES: [&str; 6] = [
    "knowledge_spaces",
    "files",
    "markdown_notes",
    "knowledge_blocks",
    "parse_jobs",
    "trash_entries",
];

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

        self.connection.execute_batch(FOUNDATION_SCHEMA)
    }

    pub fn create_knowledge_space(
        &self,
        name: &str,
        root_path: &str,
        default_permission: &str,
    ) -> rusqlite::Result<String> {
        let id = Uuid::new_v4().to_string();
        let now = OffsetDateTime::now_utc().to_string();
        self.connection.execute(
            "INSERT INTO knowledge_spaces (id, name, root_path, default_permission, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![id, name, root_path, default_permission, now],
        )?;
        Ok(id)
    }

    pub fn count_knowledge_spaces(&self) -> rusqlite::Result<u32> {
        self.connection.query_row(
            "SELECT COUNT(*) FROM knowledge_spaces WHERE deleted_at IS NULL",
            [],
            |row| row.get::<_, u32>(0),
        )
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

#[cfg(test)]
mod tests {
    use super::SqliteStore;
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
            .create_knowledge_space("面试", "D:\\知识库\\面试", "approval")
            .expect("space is inserted");

        assert!(!id.is_empty());
        assert_eq!(store.count_knowledge_spaces().unwrap(), 1);
    }

    #[test]
    fn rejects_case_only_duplicate_root_paths() {
        let store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        store
            .create_knowledge_space("面试", "D:\\知识库\\面试", "approval")
            .expect("space is inserted");

        let duplicate = store.create_knowledge_space("面试副本", "d:\\知识库\\面试", "approval");

        assert!(duplicate.is_err());
        assert_eq!(store.count_knowledge_spaces().unwrap(), 1);
    }

    #[test]
    fn soft_deleted_knowledge_space_allows_recreating_root_path() {
        let store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let deleted_id = store
            .create_knowledge_space("面试", "D:\\知识库\\面试", "approval")
            .expect("space is inserted");
        store
            .connection
            .execute(
                "UPDATE knowledge_spaces SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2",
                params![TEST_TIME, deleted_id],
            )
            .expect("space is soft deleted");

        let recreated_id = store
            .create_knowledge_space("面试新空间", "d:\\知识库\\面试", "approval")
            .expect("soft-deleted root path can be reused");

        assert_ne!(deleted_id, recreated_id);
        assert_eq!(store.count_knowledge_spaces().unwrap(), 1);
    }

    #[test]
    fn rejects_case_only_duplicate_file_paths() {
        let store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let space_id = store
            .create_knowledge_space("面试", "D:\\知识库\\文件", "approval")
            .expect("space is inserted");

        insert_file(&store, "file-1", &space_id, "README.md", "indexed").expect("file is inserted");
        let duplicate = insert_file(&store, "file-2", &space_id, "readme.md", "queued");

        assert!(duplicate.is_err());
    }

    #[test]
    fn soft_deleted_file_allows_reinserting_relative_path() {
        let store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let space_id = store
            .create_knowledge_space("面试", "D:\\知识库\\文件", "approval")
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
            .create_knowledge_space("面试", "D:\\知识库\\Redis", "approval")
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
            .create_knowledge_space("面试", "D:\\知识库\\短词", "approval")
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
            .create_knowledge_space("新空间", "d:\\知识库\\旧", "approval")
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
}
