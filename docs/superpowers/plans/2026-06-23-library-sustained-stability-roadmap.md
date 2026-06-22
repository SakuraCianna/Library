# Library Sustained Stability Roadmap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep improving the personal knowledge-base desktop app through small, reviewable modules that increase feature completeness and operational stability.

**Architecture:** Continue the local-first Tauri boundary: React displays state and explicit actions, Rust owns filesystem/database/security boundaries, Python sidecars perform bounded local parsing/OCR work, and SQLite/LanceDB remain local stores. Every module must produce a working app state, verified locally, reviewed independently, delivered by PR, merged into `main`, and then synced back to `codex/Library`.

**Tech Stack:** Windows 11, PowerShell 7, Tauri v2, React, Vite, TypeScript, CSS Modules, Rust, SQLite FTS5, LanceDB embedded mode, Python OCR sidecars, Vitest, pytest, Cargo test/check.

---

## Current Acceptance Snapshot

Recorded on 2026-06-23 from `E:\CodeHome\Library`.

- Branch: `codex/Library`.
- Module 2 started from a clean worktree at `4fbdba6ca3e23d7686aca9d883a41bba14d19240`.
- `main`, `origin/main`, `codex/Library`, and `origin/codex/Library`: `4fbdba6ca3e23d7686aca9d883a41bba14d19240`.
- File diff between `main` and `codex/Library`: none at Module 2 start.
- Frontend tests: `npm test` passed, 4 files and 29 tests.
- Frontend build: `npm run build` passed.
- OCR sidecar tests: `..\..\.venv\Scripts\python.exe -m pytest` passed, 25 tests.
- Rust format: `cargo fmt -- --check` passed.
- Rust tests: `cargo test` passed, 82 tests.
- Rust check: `cargo check` passed.
- Whitespace check: `git diff --check` passed.
- OCR local environment check: `.\scripts\检查OCR环境.ps1 -Tier medium` passed for models, sidecar, `pypdf`, `paddleocr`, and `paddlepaddle`.

## Stable Delivery Rule

Every module below must use this finish flow unless the user explicitly changes it:

- [ ] Confirm `git status --short --branch` shows work is on `codex/Library`.
- [ ] Read the current source files named by the module before editing.
- [ ] Keep changes scoped to the module files and avoid broad formatting.
- [ ] Run the module-specific tests first, then the full verification gate.
- [ ] Request an independent reviewer subagent and address blocking findings.
- [ ] Commit with a concise Chinese message only after verification and review.
- [ ] Push `codex/Library`.
- [ ] Create or update the module PR.
- [ ] Merge the PR into `main` after checks pass.
- [ ] Do not develop or commit directly on `main`; merge to `main` only through the PR flow.
- [ ] Sync local `main`, `origin/main`, `codex/Library`, and `origin/codex/Library`.
- [ ] Update README, this roadmap, and any affected plan checkboxes with only verified facts.

## Full Verification Gate

Run before every PR merge:

```powershell
Set-Location E:\CodeHome\Library\app
npm test
npm run build

Set-Location E:\CodeHome\Library\sidecars\ocr
..\..\.venv\Scripts\python.exe -m pytest

Set-Location E:\CodeHome\Library\sidecars\parser
..\..\.venv\Scripts\python.exe -m pytest

Set-Location E:\CodeHome\Library\app\src-tauri
cargo fmt -- --check
cargo test
cargo check

Set-Location E:\CodeHome\Library
git diff --check
git status --short --branch
```

For OCR-related modules, also run:

```powershell
Set-Location E:\CodeHome\Library
.\scripts\检查OCR环境.ps1 -Tier medium
```

## Module Sequence

### Module 1: Progress Ledger And Plan Hygiene

**Purpose:** Make progress tracking reliable so future agents do not confuse stale plan checkboxes with unfinished code.

**Files:**
- Modify: `README.md`
- Modify: `docs/superpowers/plans/2026-06-23-library-sustained-stability-roadmap.md`
- Inspect/possibly modify: `docs/ci-cd.md`
- Inspect: `docs/superpowers/plans/*.md`

- [x] Re-run the full verification gate and capture the exact pass/fail counts.
- [x] Compare README implemented features against source files and tests.
- [x] Mark only evidence-backed completed plan steps in older plan files.
- [x] Leave any manual smoke-test-only items unchecked unless the smoke path was run in the current module.
- [x] Add a short "当前进度验收" section to README if it improves user-facing clarity.
- [x] Finish with reviewer, commit, PR, merge, and branch sync.

### Module 2: Import And OCR Stability Limits

**Purpose:** Harden the ingestion path before adding deeper parsing: large folders, large files, too-many-page PDFs, image dimensions, cancellation, and retry behavior should fail predictably.

**Files:**
- Modify: `app/src-tauri/src/ocr.rs`
- Modify: `app/src-tauri/src/scanner/mod.rs`
- Modify: `app/src-tauri/src/state.rs`
- Modify: `app/src-tauri/src/storage/sqlite.rs`
- Modify: `sidecars/ocr/check_ocr_environment.py`
- Modify: `sidecars/ocr/ocr_sidecar.py`
- Modify: `sidecars/ocr/test_ocr_sidecar.py`
- Modify: `scripts/检查OCR环境.ps1`
- Modify: `app/src/__tests__/App.test.tsx`
- Modify: `README.md`

- [x] Add or confirm tests for PDF page limits, image size limits, and 50 MB input enforcement.
- [x] Add scanner-level guardrails for very large folders with clear Chinese user-facing messages.
- [x] Ensure cancelled scan/document/OCR jobs never write success output after cancellation.
- [x] Ensure failed document and OCR jobs can be retried without duplicate active jobs.
- [x] Show bounded error messages in the queue UI without leaking absolute private paths.
- [x] Run the full verification gate and OCR environment check.
- [x] Finish with reviewer, commit, PR, merge, and branch sync.

### Module 3: Backup Export Minimum

**Purpose:** Add a minimal recoverability layer before deeper parser work increases local data volume.

**Files:**
- Modify: `app/src-tauri/src/storage/sqlite.rs`
- Modify: `app/src-tauri/src/state.rs`
- Modify: `app/src-tauri/src/commands.rs`
- Modify: `app/src-tauri/src/lib.rs`
- Modify: `app/src/lib/tauriClient.ts`
- Modify: `app/src/App.tsx`
- Modify: `app/src/App.module.css`
- Modify: `app/src/__tests__/App.test.tsx`
- Modify: `README.md`

- [x] Add a local export command for metadata, knowledge blocks, and workspace settings.
- [x] Keep secrets, `.env`, model folders, and temporary files out of exports.
- [x] Add tests for export shape, path traversal rejection, and missing workspace behavior.
- [x] Add a simple UI entry for export status without adding restore yet.
- [x] Run the full verification gate.
- [x] Finish with reviewer, commit, PR, merge, and branch sync.

### Module 4: Document Parser Sidecar Foundation

**Purpose:** Move beyond lightweight Rust extraction for difficult PDFs and Office files while keeping all parsing local and bounded.

**Files:**
- Create: `sidecars/parser/parser_sidecar.py`
- Create: `sidecars/parser/test_parser_sidecar.py`
- Create: `sidecars/parser/requirements.txt`
- Create: `sidecars/parser/README.md`
- Modify: `app/src-tauri/src/parser.rs`
- Modify: `app/src-tauri/src/state.rs`
- Modify: `app/src-tauri/src/commands.rs`
- Modify: `app/src-tauri/tauri.conf.json`
- Modify: `README.md`
- Inspect: `app/src-tauri/src/lib.rs`

- [x] Define a stdin/stdout JSON protocol similar to the OCR sidecar.
- [x] Add tests for Markdown, text PDF fallback, DOCX, XLSX, unsupported file, timeout, and malformed output.
- [x] Keep Rust path validation and database writes as the trusted boundary.
- [x] Do not allow the parser sidecar to download models or send file contents to cloud services.
- [x] Add a development README that states required Python packages and local-only behavior.
- [x] Run parser tests, the full verification gate, and a manual smoke test with local fixtures.
- [x] Finish with reviewer, commit, PR, merge, and branch sync.

### Module 5: Backup Restore Foundation

**Purpose:** Add guarded restore after the minimal export format is stable.

**Files:**
- Modify: `app/src-tauri/src/storage/sqlite.rs`
- Modify: `app/src-tauri/src/state.rs`
- Modify: `app/src-tauri/src/commands.rs`
- Modify: `app/src-tauri/src/lib.rs`
- Modify: `app/src/types/workbench.ts`
- Modify: `app/src/lib/tauriClient.ts`
- Modify: `app/src/lib/tauriClient.test.ts`
- Modify: `app/src/hooks/useWorkbenchSnapshot.ts`
- Modify: `app/src/App.tsx`
- Modify: `app/src/App.module.css`
- Modify: `app/src/__tests__/App.test.tsx`
- Modify: `README.md`

- [x] Add a restore preflight that validates archive structure before touching existing data.
- [x] Require explicit user confirmation for restore actions because they can overwrite local app state.
- [x] Add tests for restore preflight rejection, restore path traversal rejection, and incompatible export versions.
- [x] Run the full verification gate.
- [x] Finish with reviewer, commit, PR, merge, and branch sync.

### Module 6: Agent Answer Reliability And Source Controls

**Purpose:** Improve answer quality without weakening local evidence boundaries.

**Files:**
- Modify: `app/src-tauri/src/agent.rs`
- Modify: `app/src-tauri/src/storage/sqlite.rs`
- Modify: `app/src-tauri/src/state.rs`
- Modify: `app/src/App.tsx`
- Modify: `app/src/App.module.css`
- Modify: `app/src/__tests__/App.test.tsx`
- Modify: `README.md`

- [ ] Add tests for multi-block context selection, table source ranking, OCR source ranking, and no-result answers.
- [ ] Keep DeepSeek API key redaction in Rust and never expose raw keys to the frontend.
- [ ] Add UI controls for source visibility and source-type filtering if tests show ranking ambiguity.
- [ ] Make local fallback answers clearly source-grounded and honest about missing evidence.
- [ ] Run the full verification gate.
- [ ] Finish with reviewer, commit, PR, merge, and branch sync.

### Module 7: Release Readiness

**Purpose:** Make Windows release artifacts predictable before treating the app as installable software.

**Files:**
- Modify: `.github/workflows/release.yml`
- Modify: `docs/ci-cd.md`
- Modify: `README.md`
- Inspect: `app/src-tauri/tauri.conf.json`

- [ ] Verify the release workflow still builds on Windows with the current Node, Rust, and `protoc` setup.
- [ ] Document unsigned build limitations and manual installer smoke-test steps.
- [ ] Add release artifact naming and retention expectations.
- [ ] Keep automatic update and code signing as separate modules unless signing material is available.
- [ ] Run the full verification gate and the release workflow dry run when available.
- [ ] Finish with reviewer, commit, PR, merge, and branch sync.

## Next Immediate Target

After Module 5 is delivered and merged, start Module 6: Agent Answer Reliability And Source Controls. The durable target is source-grounded answers with clearer evidence ranking across text, table, and OCR sources, plus tests that prevent no-evidence answers from sounding more certain than the local index supports.
