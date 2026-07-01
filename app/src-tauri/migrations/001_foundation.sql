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
  size_bytes INTEGER NOT NULL DEFAULT 0,
  modified_at TEXT,
  parse_status TEXT NOT NULL CHECK (parse_status IN ('indexed', 'changed', 'queued', 'failed')),
  last_scanned_at TEXT,
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

CREATE INDEX IF NOT EXISTS files_space_id_idx ON files(space_id);
CREATE INDEX IF NOT EXISTS files_parse_status_idx ON files(parse_status);
CREATE INDEX IF NOT EXISTS files_deleted_at_idx ON files(deleted_at);

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
  source_locator TEXT,
  job_type TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
  error_message TEXT,
  started_at TEXT,
  finished_at TEXT,
  progress_current INTEGER NOT NULL DEFAULT 0,
  progress_total INTEGER NOT NULL DEFAULT 0,
  phase TEXT NOT NULL DEFAULT '等待执行',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS scan_runs (
  id TEXT NOT NULL PRIMARY KEY,
  space_id TEXT NOT NULL REFERENCES knowledge_spaces(id),
  started_at TEXT NOT NULL,
  finished_at TEXT,
  status TEXT NOT NULL CHECK (status IN ('running', 'succeeded', 'failed')),
  added_count INTEGER NOT NULL DEFAULT 0,
  changed_count INTEGER NOT NULL DEFAULT 0,
  deleted_count INTEGER NOT NULL DEFAULT 0,
  failed_count INTEGER NOT NULL DEFAULT 0,
  error_message TEXT,
  started_at TEXT,
  finished_at TEXT,
  progress_current INTEGER NOT NULL DEFAULT 0,
  progress_total INTEGER NOT NULL DEFAULT 0,
  phase TEXT NOT NULL DEFAULT '等待执行',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS scan_runs (
  id TEXT NOT NULL PRIMARY KEY,
  space_id TEXT NOT NULL REFERENCES knowledge_spaces(id),
  started_at TEXT NOT NULL,
  finished_at TEXT,
  status TEXT NOT NULL CHECK (status IN ('running', 'succeeded', 'failed')),
  added_count INTEGER NOT NULL DEFAULT 0,
  changed_count INTEGER NOT NULL DEFAULT 0,
  deleted_count INTEGER NOT NULL DEFAULT 0,
  failed_count INTEGER NOT NULL DEFAULT 0,
  message TEXT
);

CREATE INDEX IF NOT EXISTS scan_runs_space_started_idx
ON scan_runs(space_id, started_at);

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

CREATE TABLE IF NOT EXISTS conversations (
  id TEXT NOT NULL PRIMARY KEY,
  space_id TEXT NOT NULL REFERENCES knowledge_spaces(id),
  title TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT
);

CREATE TABLE IF NOT EXISTS messages (
  id TEXT NOT NULL PRIMARY KEY,
  conversation_id TEXT NOT NULL REFERENCES conversations(id),
  role TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'system')),
  content TEXT NOT NULL,
  sources TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL,
  deleted_at TEXT
);
