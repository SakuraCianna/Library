# 真实文件夹接入与扫描入库实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` or `superpowers:subagent-driven-development` to execute this plan task-by-task. Track progress by updating the checkbox next to each step.

**Goal:** 把早期示例文件夹工作台推进到真实文件夹接入：用户可以选择本地文件夹，应用把文件夹登记为知识库空间，扫描支持格式文件，写入本地 SQLite 元数据，并把扫描状态展示到现有中文界面。

**Architecture:** React/Vite 只负责 UI 和调用 Tauri 命令；Rust 负责文件系统访问、路径校验、扫描、SQLite 写入和权限边界。SQLite 是本阶段唯一事实来源；LanceDB、OCR、表格深度理解和 DeepSeek 调用在后续阶段接入。

**Tech Stack:** Tauri v2, React, TypeScript, CSS Modules, Rust, rusqlite, walkdir, sha2 or blake3, Vitest, Cargo tests.

---

## Scope

本计划包含：

- 选择并登记真实文件夹。
- 扫描 `.pdf`, `.docx`, `.xlsx`, `.md`, `.txt` 文件。
- 记录文件路径、扩展名、大小、修改时间、内容指纹和解析状态。
- 对已存在文件夹做增量扫描，识别新增、变更、删除。
- 在前端展示真实文件夹列表、文件列表和扫描摘要。
- 保留现有权限语义：只读、请求批准、完全访问之间不能混用。

本计划不包含：

- OCR 模型下载与推理。
- 表格深度理解模型。
- 向量化写入 LanceDB。
- DeepSeek v4-flash 问答链路。
- 文件内容全文解析。
- 云端同步、多设备同步或账号系统。

---

## Current Files To Build On

- `app/src/App.tsx`: 中文三栏工作台和权限下拉入口。
- `app/src/hooks/useWorkbenchSnapshot.ts`: 前端状态读取 hook。
- `app/src/lib/tauriClient.ts`: Tauri 命令客户端。
- `app/src/types/workbench.ts`: 前端工作台数据类型。
- `app/src-tauri/src/commands.rs`: Tauri 命令入口。
- `app/src-tauri/src/models.rs`: Rust 领域模型和权限类型。
- `app/src-tauri/src/state.rs`: SQLite 工作台状态、权限更新和扫描协调逻辑。
- `app/src-tauri/src/storage/sqlite.rs`: SQLite 访问层。
- `app/src-tauri/migrations/001_foundation.sql`: 当前唯一本地库表基础。

---

## Data Model

新增或确认 SQLite 表字段：

- `knowledge_spaces`: `id`, `name`, `root_path`, `default_permission`, `created_at`, `updated_at`, `trashed_at`
- `knowledge_files`: `id`, `space_id`, `relative_path`, `file_name`, `extension`, `size_bytes`, `modified_at`, `content_hash`, `status`, `status_label`, `deleted_at`, `last_scanned_at`
- `scan_runs`: `id`, `space_id`, `started_at`, `finished_at`, `status`, `added_count`, `changed_count`, `deleted_count`, `failed_count`, `message`

状态约定：

- `indexed`: 文件存在且元数据已记录。
- `changed`: 文件存在但大小、修改时间或内容指纹变化。
- `queued`: 文件等待后续解析、OCR 或表格理解。
- `failed`: 扫描或元数据写入失败。

---

## Implementation Tasks

### Task 1: Add File Scanning Dependencies

**Files:**

- Modify: `app/src-tauri/Cargo.toml`
- Modify: `app/src-tauri/Cargo.lock`

- [x] Add `walkdir` for recursive traversal.
- [x] Add `blake3` or `sha2` for stable content fingerprints.
- [x] Run from `E:\CodeHome\Library\app\src-tauri`:

```powershell
cargo check
```

Expected result: Rust dependencies resolve and the current app still compiles.

### Task 2: Extend SQLite Schema

**Files:**

- Modify: `app/src-tauri/migrations/001_foundation.sql`
- Modify: `app/src-tauri/src/storage/sqlite.rs`

- [x] Add tables or missing columns for spaces, files, and scan runs.
- [x] Keep deletion as soft delete through `deleted_at`, not physical deletion.
- [x] Add indexes for `space_id`, `relative_path`, `status`, and `deleted_at`.
- [x] Add a storage test that inserts a space, upserts files, then marks a missing file as deleted.
- [x] Run from `E:\CodeHome\Library\app\src-tauri`:

```powershell
cargo test
```

Expected result: existing permission tests and new storage tests pass.

### Task 3: Add Rust Scanner Service

**Files:**

- Create: `app/src-tauri/src/scanner/mod.rs`
- Modify: `app/src-tauri/src/lib.rs`
- Modify: `app/src-tauri/src/models.rs`
- Modify: `app/src-tauri/src/state.rs`

- [x] Implement a scanner that accepts a root folder path and returns supported files only.
- [x] Store relative paths instead of duplicating absolute paths in each file row.
- [x] Skip hidden system folders and unsupported extensions.
- [x] Compute file size, modified time, and content hash.
- [x] Compare scanned files against SQLite rows to classify added, changed, unchanged, deleted, and failed.
- [x] Keep path handling Windows-safe with `PathBuf`, not string splitting.
- [x] Add unit tests for extension filtering, relative path generation, and changed-file detection.

### Task 4: Add Tauri Commands

**Files:**

- Modify: `app/src-tauri/src/commands.rs`
- Modify: `app/src-tauri/src/models.rs`
- Modify: `app/src-tauri/src/lib.rs`

- [x] Add `create_knowledge_space` command with `name`, `rootPath`, and `defaultPermission`.
- [x] Add `scan_knowledge_space` command with `spaceId`.
- [x] Add `get_workbench_snapshot` implementation backed by SQLite when data exists.
- [x] Keep an empty browser-preview fallback instead of sample knowledge data.
- [x] Return Chinese error messages at the command boundary.
- [x] Ensure `readonly` can scan and read metadata, but cannot write content changes without approval when later write operations exist.

### Task 5: Wire Frontend Client And Hook

**Files:**

- Modify: `app/src/lib/tauriClient.ts`
- Modify: `app/src/hooks/useWorkbenchSnapshot.ts`
- Modify: `app/src/types/workbench.ts`

- [x] Add typed client methods for `createKnowledgeSpace` and `scanKnowledgeSpace`.
- [x] Refresh the snapshot after a space is created or scanned.
- [x] Preserve browser-preview fallback without fake knowledge spaces.
- [x] Surface command errors in the existing status line using Chinese text.

### Task 6: Add Minimal UI Flow

**Files:**

- Modify: `app/src/App.tsx`
- Modify: `app/src/App.module.css`
- Modify: `app/src/__tests__/App.test.tsx`

- [x] Replace the current `新建` placeholder with an action that can create a knowledge folder record.
- [x] Add a compact scan action in the active folder header or file list area.
- [x] Show scan summary: indexed, changed, deleted, failed.
- [x] Keep all visible UI text Chinese.
- [x] Keep layout responsive after adding controls.
- [x] Add React tests for create/scan controls and loading/error states.

### Task 7: Verification

Run from `E:\CodeHome\Library\app`:

```powershell
npm test
npm run build
```

Run from `E:\CodeHome\Library\app\src-tauri`:

```powershell
cargo test
cargo check
```

Manual verification:

- [ ] Start with `E:\CodeHome\Library\快速启动.bat`.
- [ ] Create a test folder with `.md`, `.pdf`, `.docx`, `.xlsx`, and unsupported files.
- [ ] Add the folder as a knowledge space.
- [ ] Run scan and confirm only supported files appear.
- [ ] Modify one file and scan again; confirm it becomes `已变更`.
- [ ] Delete one file and scan again; confirm it is soft-deleted or hidden according to UI rules.
- [ ] Switch session permission through the dropdown and confirm disallowed elevation returns a Chinese error.

---

## Acceptance Criteria

- The app can register a real local folder as a knowledge space.
- The app can scan supported files and persist metadata in local SQLite.
- The workbench no longer depends on sample file data after a real scan exists.
- The UI remains fully Chinese and responsive.
- No document content, OCR result, embedding, or cloud model call is required in this phase.
- All listed automated checks pass, or failures are documented with exact error messages.

## Follow-Up Risk

This phase uses synchronous folder traversal and content hashing. Large folders should later move to a background task queue with progress, cancellation, file-size limits, and clearer failure recovery.
