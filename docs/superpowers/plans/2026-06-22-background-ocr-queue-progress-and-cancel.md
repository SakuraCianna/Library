# Background OCR Queue Progress And Cancel Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` or `superpowers:test-driven-development` before implementing this plan. Keep implementation on `codex/Library`, finish with verification, reviewer, PR, merge, and branch sync.

**Goal:** Move OCR from a manual one-job command to a bounded local background execution flow with progress, cancellation, and safer scanned-PDF limits.

**Current Baseline:** Real local PaddleOCR execution works for queued PDF jobs. The app can run the next queued OCR job, store OCR text in `knowledge_blocks`, and answer agent questions from OCR-derived text. Tauri resource sidecar resolution, local model-file validation, stdout JSON isolation, and a 50 MB PDF size cap are already in place.

---

## Scope

This module should keep the same local-first boundary and improve operational behavior:

```text
queued OCR jobs -> background worker -> progress/cancel -> bounded OCR execution -> searchable knowledge blocks
```

It should not add table extraction, handwriting recognition, GPU scheduling, distributed workers, or automatic cloud fallback.

## Key Decisions

- Rust remains the orchestrator for job state, cancellation, progress, and persistence.
- Python sidecar remains a short-lived local process per OCR job until a batch worker proves necessary.
- Cancellation should be best-effort: stop launching new OCR work, kill the active child process if possible, and never mark cancelled output as succeeded.
- Progress should start coarse-grained: queued/running/succeeded/failed/cancelled plus phase text and timestamps before adding per-page percentages.
- Limits should be explicit and user-facing: file size, page count, timeout, and unsupported file type.

## Implementation Tasks

### Task 1: Extend Parse Job State

**Files:**
- Modify: `app/src-tauri/src/models.rs`
- Modify: `app/src-tauri/src/storage/sqlite.rs`
- Modify: `app/src-tauri/migrations/001_foundation.sql`

- [ ] Add parse job fields for `started_at`, `finished_at`, `progress_current`, `progress_total`, and `phase`.
- [ ] Add storage helpers to update phase/progress and to safely cancel queued or running jobs.
- [ ] Preserve old SQLite databases through schema migration helpers.
- [ ] Add tests for state transitions, cancelled running jobs, and legacy schema upgrade.

### Task 2: Add OCR Limits

**Files:**
- Modify: `app/src-tauri/src/ocr.rs`
- Modify: `sidecars/ocr/ocr_sidecar.py`
- Modify: `sidecars/ocr/test_ocr_sidecar.py`

- [ ] Add PDF page count detection before OCR execution.
- [ ] Add a configurable MVP page limit, initially small and documented.
- [ ] Keep existing 50 MB file limit.
- [ ] Return stable errors for `OCR_INPUT_TOO_LARGE`, `OCR_TOO_MANY_PAGES`, and timeout.

### Task 3: Background Worker Command Surface

**Files:**
- Modify: `app/src-tauri/src/state.rs`
- Modify: `app/src-tauri/src/commands.rs`
- Modify: `app/src-tauri/src/lib.rs`

- [ ] Add `start_ocr_worker` command for the active space.
- [ ] Add `cancel_ocr_job` support for running jobs.
- [ ] Ensure only one OCR worker runs per knowledge space in the MVP.
- [ ] Avoid holding the SQLite mutex while a child process is running.

### Task 4: Frontend Queue UX

**Files:**
- Modify: `app/src/App.tsx`
- Modify: `app/src/App.module.css`
- Modify: `app/src/hooks/useWorkbenchSnapshot.ts`
- Modify: `app/src/lib/tauriClient.ts`
- Modify: `app/src/__tests__/App.test.tsx`

- [ ] Replace “运行 OCR” one-shot action with worker start and refresh controls.
- [ ] Show phase, progress, error message, and cancel action in the queue panel.
- [ ] Keep status text compact on narrow viewports.
- [ ] Add UI tests for running, cancellation, failure, and completed queue states.

### Task 5: Verification And Review

- [ ] Run sidecar tests, frontend tests/build, Rust fmt/test/check, diff check, and changed-file secret scan.
- [ ] Run a real local OCR smoke test with downloaded PP-OCRv6 medium models.
- [ ] Request independent reviewer subagent and fix blocking findings.
- [ ] Commit, push, create PR, merge to `main`, and sync `main`, `origin/main`, `codex/Library`, `origin/codex/Library`.

## Risks

- Killing a child process on Windows needs careful handling to avoid orphaned Paddle subprocesses.
- SQLite transaction boundaries can accidentally block UI if held across OCR execution.
- Per-page progress may not be available from PaddleOCR PDF input without custom page splitting.
- Page count detection should use a reliable library or a conservative parser, not brittle string scanning.

## Done Definition

- User can start OCR processing for a queue and cancel running work.
- Cancelled jobs never write succeeded OCR output.
- Large or too-many-page PDFs fail with useful messages.
- Queue UI shows phase/progress/error states.
- OCR output remains searchable and answerable by the agent.
- Full verification and reviewer pass.
- PR is merged to `main`, and all local/remote branches are synchronized to the final merge commit.
