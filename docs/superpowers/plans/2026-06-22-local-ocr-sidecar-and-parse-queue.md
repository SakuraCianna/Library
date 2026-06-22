# Local OCR Sidecar And Parse Queue Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a local OCR sidecar boundary and a cancellable parse queue so scanned PDFs/images can be processed without blocking the desktop UI.

**Architecture:** Keep Tauri/Rust as the trusted orchestrator for filesystem permissions, SQLite job state, progress, cancellation, and app snapshots. Add a Python sidecar under `sidecars/ocr/` for local OCR execution against the downloaded PP-OCRv6 model folders, with Rust invoking it through a strict JSON stdin/stdout protocol.

**Tech Stack:** Tauri v2, Rust, rusqlite, std process management, React/TypeScript, CSS Modules, Python 3.12, PaddleOCR/PP-OCRv6 local model assets, Vitest, Cargo tests, pytest for sidecar units.

---

## Scope

This module should produce a working local-first OCR pipeline boundary. It should not attempt high-fidelity table reconstruction, semantic table QA, GPU scheduling, or cloud OCR. It should make scanned content parseable into the same `knowledge_blocks` layer introduced by the MVP import/index/chat module.

## File Structure

- Create: `sidecars/ocr/README.md` - local setup and command contract.
- Create: `sidecars/ocr/requirements.txt` - pinned Python dependencies for OCR execution.
- Create: `sidecars/ocr/ocr_sidecar.py` - stdin/stdout JSON worker entrypoint.
- Create: `sidecars/ocr/test_ocr_sidecar.py` - unit tests for request validation and response shape.
- Create: `app/src-tauri/src/ocr.rs` - Rust sidecar command builder, JSON contract, timeout and error mapping.
- Modify: `app/src-tauri/src/models.rs` - add parse job progress, OCR request/result and cancellation models.
- Modify: `app/src-tauri/src/storage/sqlite.rs` - add parse job enqueue/list/update/cancel helpers.
- Modify: `app/src-tauri/src/state.rs` - add queue orchestration and snapshot job summaries.
- Modify: `app/src-tauri/src/commands.rs` - expose queue commands.
- Modify: `app/src-tauri/src/lib.rs` - register `ocr` module and commands.
- Modify: `app/src/types/workbench.ts` - add parse job summary types.
- Modify: `app/src/lib/tauriClient.ts` - add queue command wrappers.
- Modify: `app/src/hooks/useWorkbenchSnapshot.ts` - expose queue actions and refresh.
- Modify: `app/src/App.tsx` - show parse queue state and OCR actions.
- Modify: `app/src/App.module.css` - component-scoped queue styles.
- Modify: `README.md` - document OCR setup and MVP limitations.

---

### Task 1: Sidecar Contract Tests

**Files:**
- Create: `sidecars/ocr/test_ocr_sidecar.py`
- Create: `sidecars/ocr/ocr_sidecar.py`
- Create: `sidecars/ocr/requirements.txt`
- Create: `sidecars/ocr/README.md`

- [x] **Step 1: Add Python sidecar tests first**

Create `sidecars/ocr/test_ocr_sidecar.py`:

```python
import json

from ocr_sidecar import build_error_response, parse_request


def test_parse_request_accepts_local_file_and_model_dir():
    request = parse_request(
        json.dumps(
            {
                "filePath": "E:\\\\Knowledge\\\\scan.pdf",
                "modelDir": "E:\\\\CodeHome\\\\Library\\\\models\\\\ocr\\\\pp-ocrv6",
                "tier": "medium",
            }
        )
    )

    assert request.file_path.endswith("scan.pdf")
    assert request.model_dir.endswith("pp-ocrv6")
    assert request.tier == "medium"


def test_error_response_is_json_serializable():
    response = build_error_response("OCR_MODEL_MISSING", "模型目录不存在")

    assert response["ok"] is False
    assert response["error"]["code"] == "OCR_MODEL_MISSING"
    assert "模型目录不存在" in response["error"]["message"]
```

- [x] **Step 2: Add initial sidecar implementation**

Create `sidecars/ocr/ocr_sidecar.py`:

```python
from __future__ import annotations

from dataclasses import dataclass
import json
import sys
from pathlib import Path
from typing import Any


@dataclass(frozen=True)
class OcrRequest:
    file_path: str
    model_dir: str
    tier: str


def parse_request(raw: str) -> OcrRequest:
    payload = json.loads(raw)
    return OcrRequest(
        file_path=str(payload["filePath"]),
        model_dir=str(payload["modelDir"]),
        tier=str(payload.get("tier", "medium")),
    )


def build_error_response(code: str, message: str) -> dict[str, Any]:
    return {"ok": False, "error": {"code": code, "message": message}}


def run_ocr(request: OcrRequest) -> dict[str, Any]:
    file_path = Path(request.file_path)
    model_dir = Path(request.model_dir)
    if not file_path.is_file():
        return build_error_response("INPUT_NOT_FOUND", "输入文件不存在")
    if not model_dir.is_dir():
        return build_error_response("OCR_MODEL_MISSING", "模型目录不存在")

    return build_error_response(
        "OCR_ENGINE_NOT_INSTALLED",
        "OCR 引擎依赖尚未安装，当前只验证 sidecar 协议",
    )


def main() -> int:
    raw = sys.stdin.read()
    try:
        request = parse_request(raw)
        response = run_ocr(request)
    except Exception as exc:
        response = build_error_response("OCR_SIDECAR_ERROR", str(exc))

    sys.stdout.write(json.dumps(response, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
```

- [x] **Step 3: Add sidecar dependency file**

Create `sidecars/ocr/requirements.txt`:

```text
pytest==9.0.2
paddleocr==3.3.2
paddlepaddle==3.3.0
```

- [x] **Step 4: Add sidecar README**

Create `sidecars/ocr/README.md`:

```md
# OCR Sidecar

本目录保存本地 OCR sidecar。Rust 主进程通过 stdin 传入 JSON，通过 stdout 读取 JSON 响应。

## 本地测试

```powershell
Set-Location .\sidecars\ocr
python -m pip install -r requirements.txt
python -m pytest
```

## 请求格式

```json
{
  "filePath": "E:\\Knowledge\\scan.pdf",
  "modelDir": "E:\\CodeHome\\Library\\models\\ocr\\pp-ocrv6",
  "tier": "medium"
}
```
```

- [ ] **Step 5: Verify sidecar tests fail then pass**

Run:

```powershell
Set-Location .\sidecars\ocr
python -m pytest
```

Current verification: `..\..\.venv\Scripts\python.exe -m pytest` passes 20 OCR sidecar tests. This does not prove the historical fail-then-pass sequence, so the red/green process checkbox remains open.

---

### Task 2: Rust OCR Sidecar Boundary

**Files:**
- Create: `app/src-tauri/src/ocr.rs`
- Modify: `app/src-tauri/src/models.rs`
- Modify: `app/src-tauri/src/lib.rs`

- [x] **Step 1: Add Rust models**

Add to `app/src-tauri/src/models.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OcrSidecarRequest {
    pub file_path: String,
    pub model_dir: String,
    pub tier: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OcrSidecarResult {
    pub text: String,
    pub page_count: u32,
}
```

- [x] **Step 2: Add OCR runner tests**

Create `app/src-tauri/src/ocr.rs` with tests:

```rust
use std::path::Path;

use crate::error::AppError;
use crate::models::OcrSidecarRequest;

pub fn build_ocr_request(file_path: &Path, model_dir: &Path, tier: &str) -> OcrSidecarRequest {
    OcrSidecarRequest {
        file_path: file_path.to_string_lossy().to_string(),
        model_dir: model_dir.to_string_lossy().to_string(),
        tier: tier.to_string(),
    }
}

pub fn validate_ocr_inputs(file_path: &Path, model_dir: &Path) -> Result<(), AppError> {
    if !file_path.is_file() {
        return Err(AppError::Filesystem("OCR 输入文件不存在".to_string()));
    }
    if !model_dir.is_dir() {
        return Err(AppError::Filesystem("OCR 模型目录不存在".to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{build_ocr_request, validate_ocr_inputs};

    #[test]
    fn validates_existing_file_and_model_dir() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let input = temp_dir.path().join("scan.pdf");
        let model_dir = temp_dir.path().join("models");
        fs::write(&input, "pdf").expect("input");
        fs::create_dir(&model_dir).expect("model dir");

        validate_ocr_inputs(&input, &model_dir).expect("inputs valid");
        let request = build_ocr_request(&input, &model_dir, "medium");

        assert_eq!(request.tier, "medium");
        assert!(request.file_path.ends_with("scan.pdf"));
    }
}
```

- [x] **Step 3: Register OCR module**

Modify `app/src-tauri/src/lib.rs`:

```rust
mod ocr;
```

- [x] **Step 4: Run Rust OCR tests**

Run:

```powershell
Set-Location .\app\src-tauri
cargo test ocr
```

Expected: OCR boundary tests pass.

---

### Task 3: Parse Queue Storage

**Files:**
- Modify: `app/src-tauri/src/models.rs`
- Modify: `app/src-tauri/src/storage/sqlite.rs`

- [x] **Step 1: Add parse job model**

Add to `models.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ParseJobSummary {
    pub id: String,
    pub file_name: String,
    pub job_type: String,
    pub status: String,
    pub error_message: Option<String>,
}
```

- [x] **Step 2: Add storage tests**

Append to `storage/sqlite.rs` tests:

```rust
#[test]
fn enqueues_and_cancels_parse_job() {
    let store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
    let space_id = store
        .create_knowledge_space("OCR", "D:\\知识库\\OCR", PermissionMode::Approval)
        .expect("space is inserted");
    insert_file(&store, "file-scan", &space_id, "scan.pdf", "queued")
        .expect("file is inserted");

    let job_id = store
        .enqueue_parse_job(&space_id, "file-scan", "ocr")
        .expect("job enqueued");
    let cancelled = store.cancel_parse_job(&job_id).expect("job cancelled");
    let jobs = store.list_parse_jobs(&space_id).expect("jobs list");

    assert!(cancelled);
    assert_eq!(jobs[0].status, "cancelled");
}
```

- [x] **Step 3: Implement storage helpers**

Add methods to `SqliteStore`:

```rust
pub fn enqueue_parse_job(&self, space_id: &str, file_id: &str, job_type: &str) -> rusqlite::Result<String> {
    let id = Uuid::new_v4().to_string();
    let now = OffsetDateTime::now_utc().to_string();
    self.connection.execute(
        "INSERT INTO parse_jobs (id, space_id, file_id, job_type, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, 'queued', ?5, ?5)",
        params![id, space_id, file_id, job_type, now],
    )?;
    Ok(id)
}
```

Also implement `list_parse_jobs` and `cancel_parse_job` with explicit `queued`-only cancellation.

- [x] **Step 4: Run focused storage tests**

Run:

```powershell
Set-Location .\app\src-tauri
cargo test enqueues_and_cancels_parse_job
```

Expected: focused test passes.

---

### Task 4: UI Queue Controls

**Files:**
- Modify: `app/src/types/workbench.ts`
- Modify: `app/src/lib/tauriClient.ts`
- Modify: `app/src/hooks/useWorkbenchSnapshot.ts`
- Modify: `app/src/App.tsx`
- Modify: `app/src/App.module.css`
- Modify: `app/src/__tests__/App.test.tsx`

- [x] **Step 1: Add UI test first**

In `App.test.tsx`, add:

```ts
it("renders parse queue status when jobs exist", async () => {
  const snapshotWithJob = {
    ...snapshotWithSpace,
    parseJobs: [
      {
        id: "job-1",
        fileName: "scan.pdf",
        jobType: "ocr",
        status: "queued",
        errorMessage: null,
      },
    ],
  };

  Object.defineProperty(globalThis, "isTauri", {
    configurable: true,
    value: true,
  });
  mockIPC((cmd) => {
    if (cmd === "get_runtime_status") return runtimeStatus;
    return snapshotWithJob;
  });
  render(<App />);

  expect(await screen.findByText("解析队列")).toBeInTheDocument();
  expect(screen.getByText("scan.pdf")).toBeInTheDocument();
});
```

- [x] **Step 2: Add front-end type**

Add to `workbench.ts`:

```ts
export interface ParseJobSummary {
  id: string;
  fileName: string;
  jobType: string;
  status: string;
  errorMessage: string | null;
}
```

Add `parseJobs: ParseJobSummary[]` to `WorkbenchSnapshot`.

- [x] **Step 3: Render queue panel**

In `App.tsx`, render a small `解析队列` panel in the right content column using `snapshot.parseJobs`.

- [x] **Step 4: Add scoped styles**

In `App.module.css`, add `.queueList`, `.queueRow`, and `.queueStatus` styles using existing design tokens.

- [x] **Step 5: Run front-end checks**

Run:

```powershell
Set-Location .\app
npm test
npm run build
```

Expected: tests and build pass.

---

### Task 5: Verification, PR, Merge, Sync

**Files:**
- Modify: `README.md`

- [x] **Step 1: Update README**

Add OCR sidecar status:

```md
- 本地 OCR sidecar 协议和解析队列骨架
- 扫描版 PDF 可进入 OCR 任务队列
```

Keep high-fidelity OCR/table extraction under `暂未实现` until real inference is proven.

- [x] **Step 2: Run full verification**

Run:

```powershell
Set-Location .\app
npm test
npm run build
Set-Location .\src-tauri
cargo fmt -- --check
cargo test
cargo check
Set-Location ..\..
git diff --check
rg --hidden -n "s[k]-[A-Za-z0-9]{20,}|DEEPSEEK_API_KEY=.*s[k]-" . --glob '!.env' --glob '!.git/**' --glob '!app/node_modules/**' --glob '!app/src-tauri/target/**' --glob '!app/dist/**' --glob '!models/**'
```

Expected: all checks pass; secret scan prints no output.

- [ ] **Step 3: Request reviewer**

Ask independent reviewer to inspect OCR sidecar, queue storage, UI state, local-only model handling, and README accuracy.

- [ ] **Step 4: Commit, push, PR, merge, sync**

Run after reviewer conclusion is `通过` or `存在风险但可接受`:

```powershell
git add sidecars app README.md docs/superpowers/plans/2026-06-22-local-ocr-sidecar-and-parse-queue.md
git commit -m "接入本地 OCR sidecar 和解析队列骨架"
git push origin codex/Library
gh pr create --base main --head codex/Library --title "接入本地 OCR sidecar 和解析队列骨架" --body "..."
gh pr merge --merge --delete-branch=false
git fetch origin
git switch main
git pull --ff-only origin main
git switch codex/Library
git merge --ff-only main
git push origin codex/Library
```

Expected: `main`, `origin/main`, `codex/Library`, and `origin/codex/Library` point to the merge commit.

## Self-Review

- Spec coverage: This plan advances the next large module after import/index/chat: local OCR sidecar protocol, queue state, UI visibility, and safe merge flow.
- Placeholder scan: No task depends on undefined files or vague implementation steps.
- Type consistency: Rust and TypeScript both use `ParseJobSummary`; JSON fields are camelCase for Tauri serialization.
- Safety: Model files remain under ignored `models/`; no real key is written to docs or commits.
