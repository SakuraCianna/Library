# Real OCR Worker And Scanned PDF Ingestion Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans or superpowers:test-driven-development before implementing this plan. Keep implementation on `codex/Library`, and finish with verification, reviewer, PR, merge, and branch sync.

**Goal:** Turn the current OCR sidecar and parse queue skeleton into a real local OCR ingestion path: queued scanned PDFs are processed with local PP-OCRv6 models, converted into `ParsedDocument`, saved into `knowledge_blocks`, and become answerable in the existing agent chat.

**Current Baseline:** The app can scan supported files, parse text-like documents, index local knowledge blocks, and display OCR queue state. `models/ocr/pp-ocrv6` is Git-ignored and already supports local model discovery. The OCR sidecar currently validates the JSON protocol but intentionally returns `OCR_ENGINE_NOT_INSTALLED`.

---

## Scope

This module should implement the first real OCR execution path for scanned PDFs. It should not attempt high-fidelity page layout reconstruction, table extraction, handwriting recognition, GPU scheduling, or batch background daemons yet. The shortest accepted chain is:

```text
PDF file -> enqueue OCR -> run local sidecar -> persist OCR text as knowledge block -> ask agent about OCR content
```

## Key Decisions

- Rust remains the trusted orchestrator for file paths, model paths, queue state, cancellation, timeout, and SQLite writes.
- Python sidecar performs OCR only from local files and local model folders; it must not download models implicitly.
- CI should keep lightweight sidecar protocol tests. Heavy real OCR smoke tests should be opt-in unless a tiny deterministic fixture and fast runtime are proven stable on Windows.
- Parsed OCR text should enter the same `ParsedDocument` and `knowledge_blocks` layer as `.md`, `.txt`, `.docx`, `.xlsx`, and text PDFs.
- Runtime verification found PaddleOCR 3.7.x can consume PDF input directly with local PP-OCRv6 medium models, so the MVP does not add a separate PDF rasterization dependency.
- The Tauri command resolves the bundled resource sidecar first, then falls back to `OCR_SIDECAR_PATH` or the repository development path.

## Implementation Tasks

### Task 1: Verify PaddleOCR Runtime API

**Files:**
- Modify: `sidecars/ocr/requirements.txt`
- Modify: `sidecars/ocr/README.md`
- Modify: `sidecars/ocr/test_ocr_sidecar.py`

- [x] Check the currently installed PaddleOCR API on Windows before coding the adapter.
- [x] Confirm how to bind local PP-OCRv6 det/rec model directories without automatic download.
- [x] Strictly validate local model asset files before PaddleOCR starts.
- [x] Add a unit-testable adapter boundary so protocol tests do not import heavy OCR modules by default.
- [x] Document runtime dependency install separately from lightweight protocol tests.

### Task 2: Add PDF Page Rasterization Boundary

**Files:**
- Modify: `sidecars/ocr/requirements.txt`
- Modify: `sidecars/ocr/ocr_sidecar.py`
- Create: `sidecars/ocr/test_pdf_pages.py`

- [x] Verify a separate rasterization dependency is not needed for the MVP because PaddleOCR accepts PDF input directly.
- [x] Add an MVP PDF file size limit before adding batch OCR.
- [ ] Add explicit page count and image size limits before adding batch OCR.

### Task 3: Implement OCR Text Extraction

**Files:**
- Modify: `sidecars/ocr/ocr_sidecar.py`
- Modify: `sidecars/ocr/test_ocr_sidecar.py`

- [x] Return successful JSON with `text`, `pageCount`, and basic per-page metadata.
- [x] Return stable error codes for missing model, unsupported file, empty OCR result, timeout candidate, and runtime import failure.
- [x] Keep stdout as JSON only; logs must go to stderr.
- [x] Add tests for response shape, empty output handling, and local model directory validation.

### Task 4: Add Rust Sidecar Process Runner

**Files:**
- Modify: `app/src-tauri/src/ocr.rs`
- Modify: `app/src-tauri/src/models.rs`
- Modify: `app/src-tauri/src/state.rs`

- [x] Spawn the Python sidecar with strict stdin/stdout JSON.
- [x] Resolve bundled Tauri resource sidecar, explicit `OCR_SIDECAR_PATH`, and development fallback paths.
- [x] Add timeout and map sidecar errors into `AppError`.
- [x] Avoid shell string construction for paths; pass process args and stdin directly.
- [x] Unit test JSON decode and sidecar error mapping.

### Task 5: Process Queued OCR Jobs Into Knowledge Blocks

**Files:**
- Modify: `app/src-tauri/src/storage/sqlite.rs`
- Modify: `app/src-tauri/src/state.rs`
- Modify: `app/src-tauri/src/parser.rs` or create `app/src-tauri/src/parser/ocr.rs`

- [x] Add storage helpers to mark jobs `running`, `succeeded`, and `failed`.
- [x] For an OCR job, build a `ParsedDocument` with title, OCR body, summary, and source locator.
- [x] Save OCR output through `replace_file_knowledge_block`.
- [x] Update file parse status consistently after OCR success or failure.
- [x] Keep duplicate active OCR job protection from the current module.

### Task 6: UI Execute/Refresh Flow

**Files:**
- Modify: `app/src-tauri/src/commands.rs`
- Modify: `app/src-tauri/src/lib.rs`
- Modify: `app/src/lib/tauriClient.ts`
- Modify: `app/src/hooks/useWorkbenchSnapshot.ts`
- Modify: `app/src/App.tsx`
- Modify: `app/src/App.module.css`
- Modify: `app/src/__tests__/App.test.tsx`

- [x] Add a command to run the next queued OCR job for the active space.
- [x] Surface queued, running, succeeded, failed, and cancelled states in the existing queue panel.
- [x] Keep text inside buttons stable at desktop and narrow widths.
- [x] Add UI tests for running a queued OCR job and displaying successful OCR snapshot output.

### Task 7: End-To-End Local Fixture

**Files:**
- Create: `fixtures/ocr/README.md`
- Create or add: a tiny generated scanned PDF fixture only if license-safe and small.
- Modify: Rust/Python tests as needed.

- [x] Document a manual local smoke path instead of committing a binary fixture.
- [x] Verify the chain: enqueue -> OCR -> `knowledge_blocks` -> agent answer includes OCR text.

## Verification Gate

Run before requesting review:

```powershell
Set-Location .\sidecars\ocr
python -m pytest
Set-Location ..\..\app
npm test
npm run build
Set-Location .\src-tauri
cargo fmt -- --check
cargo test
cargo check
Set-Location ..\..
git diff --check
```

Also run a real local smoke test with a scanned PDF and the downloaded PP-OCRv6 medium model folders. Do not claim this smoke test passed unless it was run in the current turn.

## Review Focus

- No implicit model download
- No shell-injection path handling
- No duplicate active OCR jobs
- Cancellation does not mark succeeded output
- OCR failures preserve useful error messages
- Parsed OCR text is searchable and answerable through the existing chat path
- Heavy dependencies are documented and do not slow CI unexpectedly

## Risks

- PaddleOCR Windows runtime may require large dependencies and could be slow on CPU.
- PDF rasterization dependency choice affects install reliability.
- OCR output quality may be poor without orientation and layout post-processing.
- Running OCR synchronously from a Tauri command can block UX if not bounded by explicit job execution and timeout.

## Done Definition

- A scanned PDF can be OCR processed locally with downloaded models.
- OCR result is stored as a searchable knowledge block.
- The agent can answer a question using OCR-derived text.
- Full verification passes.
- Independent reviewer conclusion is `通过` or accepted with documented residual risk.
- PR is merged to `main`, and `main`, `origin/main`, `codex/Library`, and `origin/codex/Library` all point to the final merge commit.
