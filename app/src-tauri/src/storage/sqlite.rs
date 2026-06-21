use std::path::Path;

use rusqlite::{params, Connection};
use time::OffsetDateTime;
use uuid::Uuid;

pub struct SqliteStore {
    connection: Connection,
}

impl SqliteStore {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let connection = Connection::open(path)?;
        let store = Self { connection };
        store.apply_foundation_schema()?;
        Ok(store)
    }

    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let connection = Connection::open_in_memory()?;
        let store = Self { connection };
        store.apply_foundation_schema()?;
        Ok(store)
    }

    fn apply_foundation_schema(&self) -> rusqlite::Result<()> {
        self.connection
            .execute_batch(include_str!("../../migrations/001_foundation.sql"))
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
}

#[cfg(test)]
mod tests {
    use super::SqliteStore;
    use rusqlite::params;

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
    fn rejects_case_only_duplicate_file_paths() {
        let store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let space_id = store
            .create_knowledge_space("面试", "D:\\知识库\\文件", "approval")
            .expect("space is inserted");

        store
            .connection
            .execute(
                "INSERT INTO files (
                    id, space_id, relative_path, extension, parse_status, created_at, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                params![
                    "file-1",
                    space_id,
                    "README.md",
                    "md",
                    "indexed",
                    "2026-06-21T00:00:00Z"
                ],
            )
            .expect("file is inserted");

        let duplicate = store.connection.execute(
            "INSERT INTO files (
                id, space_id, relative_path, extension, parse_status, created_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![
                "file-2",
                space_id,
                "readme.md",
                "md",
                "queued",
                "2026-06-21T00:00:00Z"
            ],
        );

        assert!(duplicate.is_err());
    }

    #[test]
    fn indexes_chinese_knowledge_blocks_in_fts() {
        let store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let space_id = store
            .create_knowledge_space("面试", "D:\\知识库\\Redis", "approval")
            .expect("space is inserted");

        store
            .connection
            .execute(
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
                    "2026-06-21T00:00:00Z"
                ],
            )
            .expect("block is inserted");

        let hits = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM knowledge_blocks_fts WHERE knowledge_blocks_fts MATCH ?1",
                ["缓存穿透"],
                |row| row.get::<_, u32>(0),
            )
            .expect("fts query works");

        assert_eq!(hits, 1);
    }
}
