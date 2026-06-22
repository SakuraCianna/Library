# MVP Import Index Chat Chain Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the shortest local-first chain for importing supported documents, creating structured knowledge blocks and summaries, then answering sidebar chat questions from indexed content.

**Architecture:** Continue the existing Tauri v2 + React/Vite + Rust core. Rust owns file parsing, SQLite writes, FTS search, DeepSeek request construction, and secret loading; React only triggers explicit actions and renders returned state.

**Tech Stack:** Tauri v2 commands, React 19, TypeScript, CSS Modules, Rust, rusqlite/SQLite FTS5, reqwest for DeepSeek Chat Completions, zip for lightweight DOCX/XLSX XML extraction, Vitest, Cargo tests.

---

## File Structure

- Modify: `app/src-tauri/Cargo.toml` - add HTTP and ZIP parsing dependencies.
- Create: `app/src-tauri/src/parser.rs` - parse `.md`, `.txt`, `.docx`, `.xlsx`, and lightweight text PDFs into a unified `ParsedDocument`.
- Create: `app/src-tauri/src/agent.rs` - build grounded answers from local search hits and optionally call DeepSeek with redacted local config.
- Modify: `app/src-tauri/src/runtime.rs` - load `.env` from the project ancestry without logging secrets.
- Modify: `app/src-tauri/src/models.rs` - add index, chat, parse, and source-reference contracts.
- Modify: `app/src-tauri/src/storage/sqlite.rs` - persist parsed documents as `knowledge_blocks`, update file status, and search blocks.
- Modify: `app/src-tauri/src/state.rs` - add `index_knowledge_space` and `ask_agent` orchestration.
- Modify: `app/src-tauri/src/commands.rs` and `app/src-tauri/src/lib.rs` - expose Tauri commands.
- Modify: `app/src/types/workbench.ts`, `app/src/lib/tauriClient.ts`, `app/src/hooks/useWorkbenchSnapshot.ts` - add front-end contracts and actions.
- Modify: `app/src/App.tsx` and `app/src/App.module.css` - add index button and working chat composer.
- Modify: `app/src/__tests__/App.test.tsx` and Rust unit tests - cover the shortest chain.
- Create: `.env` - local-only DeepSeek runtime values with Chinese comments.
- Modify: `README.md` - document the MVP chain accurately without exposing the real key.

## Tasks

### Task 1: Lock Behavior With Failing Tests

- [ ] Add Rust tests showing queued Markdown files can be parsed, stored as searchable knowledge blocks, and used by `ask_agent`.
- [ ] Add a React test showing the sidebar composer sends a question through `ask_agent` and renders the returned answer.
- [ ] Run `cargo test parser storage::sqlite state::tests::indexes_scanned_files_into_searchable_blocks` and `npm test -- App.test.tsx`; confirm failures are caused by missing implementation.

### Task 2: Implement Structured Parsing And Indexing

- [ ] Add `ParsedDocument`, parser functions, and lightweight extractors for `.md`, `.txt`, `.docx`, `.xlsx`, and `.pdf`.
- [ ] Add SQLite methods to list parse candidates, replace file knowledge blocks, mark status indexed or failed, and search FTS/fallback text.
- [ ] Add `index_knowledge_space` command that parses queued/changed files and refreshes the workbench snapshot.

### Task 3: Implement Agent Chat

- [ ] Add local `.env` loading for DeepSeek settings while preserving OS environment precedence.
- [ ] Add `ask_agent` command that searches local blocks, builds source-grounded context, calls DeepSeek when configured, and falls back to a local cited answer on API failure or missing key.
- [ ] Keep chat messages in memory for the active desktop session and return them through the existing snapshot.

### Task 4: Wire Frontend Controls

- [ ] Add an index/summary button beside scan, wired to `index_knowledge_space`.
- [ ] Make the sidebar form controlled, submit via `ask_agent`, show disabled/loading states, and preserve current messages.
- [ ] Keep styles inside `App.module.css` and preserve the existing workbench layout.

### Task 5: Document And Verify

- [ ] Write `.env` with Chinese comments and the provided local DeepSeek key.
- [ ] Update README feature status and quick-start notes without writing the real key.
- [ ] Run `npm test`, `npm run build`, `cargo fmt -- --check`, `cargo test`, `cargo check` when possible, plus a secret scan that excludes `.env`.
- [ ] Request an independent reviewer and fix or report findings.

## Self-Review

- Spec coverage: This plan covers the requested MVP path: supported import formats, unified local structured layer, summary/index action, and sidebar agent chat over indexed content.
- Scope control: Full OCR, high-fidelity PDF layout parsing, streaming output, long-running background jobs, and vector retrieval remain deferred.
- Safety: `.env` is ignored; README and final report must not include the real API key.
