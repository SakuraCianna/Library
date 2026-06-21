PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS knowledge_spaces (
  id TEXT NOT NULL PRIMARY KEY,
  name TEXT NOT NULL,
  root_path TEXT NOT NULL COLLATE NOCASE,
  default_permission TEXT NOT NULL CHECK (default_permission IN ('readonly', 'approval', 'full')),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT
);

CREATE TABLE IF NOT EXISTS files (
  id TEXT NOT NULL PRIMARY KEY,
  space_id TEXT NOT NULL REFERENCES knowledge_spaces(id),
  relative_path TEXT NOT NULL COLLATE NOCASE,
  extension TEXT NOT NULL,
  content_hash TEXT,
  modified_at TEXT,
  parse_status TEXT NOT NULL CHECK (parse_status IN ('indexed', 'changed', 'queued', 'failed')),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT
);

CREATE TABLE IF NOT EXISTS markdown_notes (
  id TEXT NOT NULL PRIMARY KEY,
  file_id TEXT REFERENCES files(id),
  space_id TEXT NOT NULL REFERENCES knowledge_spaces(id),
  relative_path TEXT NOT NULL COLLATE NOCASE,
  user_editable INTEGER NOT NULL DEFAULT 1 CHECK (user_editable IN (0, 1)),
  last_generated_hash TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT
);

CREATE TABLE IF NOT EXISTS knowledge_blocks (
  fts_rowid INTEGER PRIMARY KEY,
  id TEXT NOT NULL UNIQUE,
  space_id TEXT NOT NULL REFERENCES knowledge_spaces(id),
  file_id TEXT REFERENCES files(id),
  note_id TEXT REFERENCES markdown_notes(id),
  title TEXT NOT NULL,
  body TEXT NOT NULL,
  source_kind TEXT NOT NULL CHECK (source_kind IN ('original_file', 'markdown_note', 'table')),
  source_locator TEXT NOT NULL,
  searchable INTEGER NOT NULL DEFAULT 1 CHECK (searchable IN (0, 1)),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS knowledge_spaces_active_root_path_idx
ON knowledge_spaces(root_path COLLATE NOCASE)
WHERE deleted_at IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS files_active_relative_path_idx
ON files(space_id, relative_path COLLATE NOCASE)
WHERE deleted_at IS NULL;

CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_blocks_fts USING fts5(
  title,
  body,
  content='knowledge_blocks',
  content_rowid='fts_rowid',
  tokenize='trigram'
);

CREATE TRIGGER IF NOT EXISTS knowledge_blocks_fts_ai
AFTER INSERT ON knowledge_blocks
BEGIN
  INSERT INTO knowledge_blocks_fts(rowid, title, body)
  SELECT new.fts_rowid, new.title, new.body
  WHERE new.searchable = 1 AND new.deleted_at IS NULL;
END;

CREATE TRIGGER IF NOT EXISTS knowledge_blocks_fts_ad
AFTER DELETE ON knowledge_blocks
BEGIN
  INSERT INTO knowledge_blocks_fts(knowledge_blocks_fts, rowid, title, body)
  SELECT 'delete', old.fts_rowid, old.title, old.body
  WHERE old.searchable = 1 AND old.deleted_at IS NULL;
END;

CREATE TRIGGER IF NOT EXISTS knowledge_blocks_fts_au
AFTER UPDATE ON knowledge_blocks
BEGIN
  INSERT INTO knowledge_blocks_fts(knowledge_blocks_fts, rowid, title, body)
  SELECT 'delete', old.fts_rowid, old.title, old.body
  WHERE old.searchable = 1 AND old.deleted_at IS NULL;

  INSERT INTO knowledge_blocks_fts(rowid, title, body)
  SELECT new.fts_rowid, new.title, new.body
  WHERE new.searchable = 1 AND new.deleted_at IS NULL;
END;

CREATE TABLE IF NOT EXISTS parse_jobs (
  id TEXT NOT NULL PRIMARY KEY,
  space_id TEXT NOT NULL REFERENCES knowledge_spaces(id),
  file_id TEXT REFERENCES files(id),
  job_type TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
  error_message TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS trash_entries (
  id TEXT NOT NULL PRIMARY KEY,
  space_id TEXT NOT NULL REFERENCES knowledge_spaces(id),
  entity_kind TEXT NOT NULL CHECK (entity_kind IN ('file', 'markdown_note', 'knowledge_block')),
  entity_id TEXT NOT NULL,
  display_name TEXT NOT NULL,
  original_locator TEXT NOT NULL,
  deleted_at TEXT NOT NULL,
  restored_at TEXT
);
