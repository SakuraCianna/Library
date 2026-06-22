# AI Runtime And Local OCR Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a secret-safe DeepSeek runtime status layer and a local OCR model download/status foundation without committing API keys or model binaries.

**Architecture:** Keep secrets and filesystem checks inside the Rust/Tauri core, expose only redacted runtime status to React, and keep model downloads in ignored local folders. The first usable OCR foundation downloads PP-OCRv6 detection and recognition models locally; richer table/document understanding is represented as a later pipeline boundary rather than mixed into the current scanner.

**Tech Stack:** Tauri v2, Rust, React, TypeScript, CSS Modules, SQLite metadata, Iconify, PowerShell, DeepSeek OpenAI-compatible API, PaddleOCR PP-OCRv6 local model assets.

---

## Source Notes

- DeepSeek official docs currently list `deepseek-v4-flash` and `deepseek-v4-pro`, with `https://api.deepseek.com` as the OpenAI-compatible base URL.
- DeepSeek official pricing docs mark `deepseek-chat` and `deepseek-reasoner` as legacy aliases to be deprecated on 2026-07-24 15:59 UTC.
- PaddleOCR official release notes describe PP-OCRv6 as the current OCR line, with tiny/small/medium tiers and Hugging Face/ModelScope availability.
- Use PP-OCRv6 medium as the default local OCR tier for better quality; allow switching to small/tiny later if local CPU performance is poor.

## File Structure

- Modify: `.gitignore` - ignore local env files and downloaded model folders while keeping examples tracked.
- Create: `.env.example` - document local-only runtime variables with placeholder values and Chinese comments.
- Create: `scripts/下载OCR模型.ps1` - download PP-OCRv6 model repositories into an ignored local folder.
- Create: `app/src-tauri/src/runtime.rs` - read local runtime config, redact secret values, and detect OCR model folders.
- Modify: `app/src-tauri/src/models.rs` - add serializable runtime status types.
- Modify: `app/src-tauri/src/commands.rs` - expose `get_runtime_status`.
- Modify: `app/src-tauri/src/lib.rs` - register the runtime module and Tauri command.
- Modify: `app/src/types/workbench.ts` - add front-end runtime status types.
- Modify: `app/src/lib/tauriClient.ts` - call `get_runtime_status` and return a browser-safe fallback.
- Create: `app/src/hooks/useRuntimeStatus.ts` - fetch runtime status for the settings panel.
- Modify: `app/src/App.tsx` - show runtime status under the existing left-side settings gear.
- Modify: `app/src/App.module.css` - style the runtime status panel with existing design tokens.
- Modify: `README.md` - document safe DeepSeek and OCR model setup without writing secrets.
- Test: `app/src-tauri/src/runtime.rs` unit tests and `app/src/__tests__/App.test.tsx` UI coverage.

### Task 1: Secret And Model Ignore Rules

**Files:**
- Modify: `.gitignore`
- Create: `.env.example`

- [ ] **Step 1: Add a failing repository safety check**

Run:

```powershell
git status --short --branch
Select-String -Path .\.gitignore -Pattern '^\.env$','^\.env\.\*$','^models/$'
Test-Path .\.env.example
```

Expected: `Select-String` does not find every required ignore pattern, and `.env.example` is missing.

- [x] **Step 2: Update `.gitignore`**

Append this exact block to `.gitignore`:

```gitignore
.env
.env.*
!.env.example
models/
app/models/
app/.models/
app/src-tauri/models/
```

- [x] **Step 3: Add `.env.example`**

Create `.env.example` with this content:

```dotenv
# DeepSeek API Key, 仅本机使用, 不要提交真实密钥
DEEPSEEK_API_KEY=

# DeepSeek 模型名, 默认只使用 deepseek-v4-flash
DEEPSEEK_MODEL=deepseek-v4-flash

# DeepSeek OpenAI 兼容接口地址, 默认官方地址
DEEPSEEK_BASE_URL=https://api.deepseek.com

# 本地 OCR 模型目录, 建议使用项目根目录 models/ocr 或用户数据目录
OCR_MODEL_DIR=

# 本地 OCR 模型规格, 默认使用 PP-OCRv6 medium
OCR_MODEL_TIER=medium
```

- [x] **Step 4: Verify ignore behavior**

Run:

```powershell
git check-ignore .env .env.local models/ocr/sample.bin app/models/ocr/sample.bin
git check-ignore .env.example
```

Expected: first command prints ignored paths. Second command exits non-zero and prints nothing because `.env.example` remains tracked.

### Task 2: OCR Model Download Script

**Files:**
- Create: `scripts/下载OCR模型.ps1`

- [x] **Step 1: Write the downloader script**

Create `scripts/下载OCR模型.ps1`:

```powershell
param(
  [ValidateSet('tiny', 'small', 'medium')]
  [string]$Tier = 'medium',
  [string]$TargetDir = "$PSScriptRoot\..\models\ocr\pp-ocrv6"
)

$ErrorActionPreference = 'Stop'
$targetItem = New-Item -ItemType Directory -Force -Path $TargetDir
$target = $targetItem.FullName
$repos = @(
  "PaddlePaddle/PP-OCRv6_${Tier}_det",
  "PaddlePaddle/PP-OCRv6_${Tier}_rec"
)
$pythonCommand = Get-Command py -ErrorAction SilentlyContinue

if ($null -eq $pythonCommand) {
  $pythonCommand = Get-Command python -ErrorAction Stop
}

& $pythonCommand.Source -c "import huggingface_hub" 2>$null
if ($LASTEXITCODE -ne 0) {
  & $pythonCommand.Source -m pip install --upgrade huggingface_hub
}

try {
  foreach ($repo in $repos) {
    $name = ($repo -split '/')[1]
    $out = Join-Path $target $name
    $env:HF_REPO_ID = $repo
    $env:HF_LOCAL_DIR = $out
    & $pythonCommand.Source -c "import os; from huggingface_hub import snapshot_download; snapshot_download(repo_id=os.environ['HF_REPO_ID'], local_dir=os.environ['HF_LOCAL_DIR'], local_dir_use_symlinks=False)"
    if ($LASTEXITCODE -ne 0) {
      throw "OCR 模型下载失败: $repo"
    }
  }
}
finally {
  Remove-Item Env:\HF_REPO_ID -ErrorAction SilentlyContinue
  Remove-Item Env:\HF_LOCAL_DIR -ErrorAction SilentlyContinue
}

Write-Host "OCR 模型已下载到: $target"
```

- [x] **Step 2: Validate the script syntax without downloading**

Run:

```powershell
$null = [System.Management.Automation.Language.Parser]::ParseFile("$PWD\scripts\下载OCR模型.ps1", [ref]$null, [ref]$null)
```

Expected: no parser errors.

### Task 3: Runtime Status Types And Rust Core

**Files:**
- Create: `app/src-tauri/src/runtime.rs`
- Modify: `app/src-tauri/src/models.rs`

- [x] **Step 1: Add Rust model types**

Add these types to `app/src-tauri/src/models.rs`:

```rust
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub deepseek: DeepSeekRuntimeStatus,
    pub ocr: OcrRuntimeStatus,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepSeekRuntimeStatus {
    pub configured: bool,
    pub model: String,
    pub base_url: String,
    pub key_hint: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OcrRuntimeStatus {
    pub configured: bool,
    pub tier: String,
    pub model_dir: String,
    pub missing_models: Vec<String>,
}
```

- [x] **Step 2: Create the runtime module**

Create `app/src-tauri/src/runtime.rs`:

```rust
use std::env;
use std::path::{Path, PathBuf};

use crate::models::{DeepSeekRuntimeStatus, OcrRuntimeStatus, RuntimeStatus};

const DEFAULT_DEEPSEEK_MODEL: &str = "deepseek-v4-flash";
const DEFAULT_DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";
const DEFAULT_OCR_TIER: &str = "medium";

pub fn runtime_status(app_data_dir: &Path) -> RuntimeStatus {
    let api_key = env::var("DEEPSEEK_API_KEY").ok();
    let model = env::var("DEEPSEEK_MODEL").unwrap_or_else(|_| DEFAULT_DEEPSEEK_MODEL.to_string());
    let base_url =
        env::var("DEEPSEEK_BASE_URL").unwrap_or_else(|_| DEFAULT_DEEPSEEK_BASE_URL.to_string());
    let tier = env::var("OCR_MODEL_TIER").unwrap_or_else(|_| DEFAULT_OCR_TIER.to_string());
    let model_dir = env::var("OCR_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| app_data_dir.join("models").join("ocr").join("pp-ocrv6"));

    build_runtime_status(api_key.as_deref(), model, base_url, tier, model_dir)
}

fn build_runtime_status(
    api_key: Option<&str>,
    model: String,
    base_url: String,
    tier: String,
    model_dir: PathBuf,
) -> RuntimeStatus {
    let required_models = [
        format!("PP-OCRv6_{tier}_det"),
        format!("PP-OCRv6_{tier}_rec"),
    ];
    let missing_models = required_models
        .iter()
        .filter(|name| !model_dir.join(name).is_dir())
        .cloned()
        .collect::<Vec<_>>();

    RuntimeStatus {
        deepseek: DeepSeekRuntimeStatus {
            configured: api_key.filter(|value| !value.trim().is_empty()).is_some(),
            model,
            base_url,
            key_hint: api_key.map(redact_key).unwrap_or_else(|| "未配置".to_string()),
        },
        ocr: OcrRuntimeStatus {
            configured: missing_models.is_empty(),
            tier,
            model_dir: model_dir.to_string_lossy().to_string(),
            missing_models,
        },
    }
}

fn redact_key(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() <= 8 {
        return "已配置".to_string();
    }

    format!("{}...{}", &trimmed[..3], &trimmed[trimmed.len() - 4..])
}
```

- [x] **Step 3: Add Rust tests**

Add this test module to `runtime.rs`:

```rust
#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::build_runtime_status;

    #[test]
    fn redacts_deepseek_key_and_reports_missing_ocr_models() {
        let temp_dir = tempdir().expect("temp dir");
        let status = build_runtime_status(
            Some("sk-test-12345678"),
            "deepseek-v4-flash".to_string(),
            "https://api.deepseek.com".to_string(),
            "medium".to_string(),
            temp_dir.path().to_path_buf(),
        );

        assert!(status.deepseek.configured);
        assert_eq!(status.deepseek.key_hint, "sk-...5678");
        assert!(!status.ocr.configured);
        assert_eq!(status.ocr.missing_models.len(), 2);
    }

    #[test]
    fn detects_downloaded_ocr_model_folders() {
        let temp_dir = tempdir().expect("temp dir");
        std::fs::create_dir(temp_dir.path().join("PP-OCRv6_medium_det")).expect("det dir");
        std::fs::create_dir(temp_dir.path().join("PP-OCRv6_medium_rec")).expect("rec dir");

        let status = build_runtime_status(
            None,
            "deepseek-v4-flash".to_string(),
            "https://api.deepseek.com".to_string(),
            "medium".to_string(),
            temp_dir.path().to_path_buf(),
        );

        assert!(!status.deepseek.configured);
        assert_eq!(status.deepseek.key_hint, "未配置");
        assert!(status.ocr.configured);
        assert!(status.ocr.missing_models.is_empty());
    }
}
```

- [x] **Step 4: Run the focused Rust test**

Run:

```powershell
Set-Location .\app\src-tauri
cargo test runtime
```

Expected: runtime tests pass.

### Task 4: Tauri Command Boundary

**Files:**
- Modify: `app/src-tauri/src/commands.rs`
- Modify: `app/src-tauri/src/lib.rs`

- [x] **Step 1: Add command implementation**

Add this command to `commands.rs`:

```rust
#[tauri::command]
pub fn get_runtime_status(app: tauri::AppHandle) -> Result<RuntimeStatus, ErrorResponse> {
    let app_data_dir = app.path().app_data_dir().map_err(|error| ErrorResponse {
        message: format!("无法读取应用数据目录：{error}"),
    })?;

    Ok(crate::runtime::runtime_status(&app_data_dir))
}
```

Also import:

```rust
use tauri::Manager;
use crate::error::ErrorResponse;
use crate::models::RuntimeStatus;
```

- [x] **Step 2: Register runtime module and command**

In `lib.rs`, add:

```rust
mod runtime;
```

Register command:

```rust
commands::get_runtime_status
```

- [x] **Step 3: Run command compilation check**

Run:

```powershell
Set-Location .\app\src-tauri
$env:PROTOC = "E:\CodeHome\Library\app\src-tauri\target\protoc\bin\protoc.exe"
cargo check
```

Expected: compile succeeds and no API key appears in output.

### Task 5: Front-End Runtime Status Hook

**Files:**
- Modify: `app/src/types/workbench.ts`
- Modify: `app/src/lib/tauriClient.ts`
- Create: `app/src/hooks/useRuntimeStatus.ts`

- [x] **Step 1: Add TypeScript types**

Add to `app/src/types/workbench.ts`:

```ts
export interface RuntimeStatus {
  deepseek: DeepSeekRuntimeStatus;
  ocr: OcrRuntimeStatus;
}

export interface DeepSeekRuntimeStatus {
  configured: boolean;
  model: string;
  baseUrl: string;
  keyHint: string;
}

export interface OcrRuntimeStatus {
  configured: boolean;
  tier: string;
  modelDir: string;
  missingModels: string[];
}
```

- [x] **Step 2: Add Tauri client fallback**

Add to `tauriClient.ts`:

```ts
import type { PermissionMode, RuntimeStatus, WorkbenchSnapshot } from "../types/workbench";

const browserRuntimeStatus: RuntimeStatus = {
  deepseek: {
    configured: false,
    model: "deepseek-v4-flash",
    baseUrl: "https://api.deepseek.com",
    keyHint: "桌面端读取本机配置",
  },
  ocr: {
    configured: false,
    tier: "medium",
    modelDir: "桌面端读取本机模型目录",
    missingModels: ["PP-OCRv6_medium_det", "PP-OCRv6_medium_rec"],
  },
};

export async function getRuntimeStatus(): Promise<RuntimeStatus> {
  if (!isTauriRuntime()) {
    return browserRuntimeStatus;
  }

  return invoke<RuntimeStatus>("get_runtime_status");
}
```

- [x] **Step 3: Create `useRuntimeStatus.ts`**

```ts
import { useEffect, useState } from "react";

import { getRuntimeStatus } from "../lib/tauriClient";
import type { RuntimeStatus } from "../types/workbench";

interface RuntimeStatusState {
  runtimeStatus: RuntimeStatus | null;
  runtimeStatusError: string | null;
}

export function useRuntimeStatus(): RuntimeStatusState {
  const [state, setState] = useState<RuntimeStatusState>({
    runtimeStatus: null,
    runtimeStatusError: null,
  });

  useEffect(() => {
    let active = true;

    getRuntimeStatus()
      .then((runtimeStatus) => {
        if (active) {
          setState({ runtimeStatus, runtimeStatusError: null });
        }
      })
      .catch(() => {
        if (active) {
          setState({
            runtimeStatus: null,
            runtimeStatusError: "运行配置读取失败",
          });
        }
      });

    return () => {
      active = false;
    };
  }, []);

  return state;
}
```

- [x] **Step 4: Run front-end typecheck through build**

Run:

```powershell
Set-Location .\app
npm run build
```

Expected: TypeScript build succeeds.

### Task 6: Settings Panel UI

**Files:**
- Modify: `app/src/App.tsx`
- Modify: `app/src/App.module.css`
- Modify: `app/src/__tests__/App.test.tsx`

- [x] **Step 1: Write a UI test for runtime status**

Add this expectation to the empty-state render test in `App.test.tsx`:

```ts
expect(await screen.findByText("DeepSeek")).toBeInTheDocument();
expect(screen.getByText("deepseek-v4-flash")).toBeInTheDocument();
expect(screen.getByText("本地 OCR")).toBeInTheDocument();
```

- [ ] **Step 2: Run the focused test and observe failure**

Run:

```powershell
Set-Location .\app
npm test -- App.test.tsx
```

Expected: fails because runtime status is not rendered yet.

- [x] **Step 3: Render runtime status in `App.tsx`**

Import the hook:

```ts
import { useRuntimeStatus } from "./hooks/useRuntimeStatus";
```

Inside `App`, call:

```ts
const { runtimeStatus, runtimeStatusError } = useRuntimeStatus();
```

Render this block inside `showDefaultPermissionHelp` after permission copy:

```tsx
<div className={styles.runtimeStatus}>
  <div className={styles.runtimeRow}>
    <span>DeepSeek</span>
    <strong>{runtimeStatus?.deepseek.model ?? "deepseek-v4-flash"}</strong>
  </div>
  <div className={styles.runtimeMeta}>
    {runtimeStatus?.deepseek.configured ? `密钥 ${runtimeStatus.deepseek.keyHint}` : "密钥未配置"}
  </div>
  <div className={styles.runtimeRow}>
    <span>本地 OCR</span>
    <strong>{runtimeStatus?.ocr.configured ? "已就绪" : "未就绪"}</strong>
  </div>
  <div className={styles.runtimeMeta}>
    {runtimeStatusError ??
      (runtimeStatus?.ocr.configured
        ? `模型目录 ${runtimeStatus.ocr.modelDir}`
        : `缺少 ${runtimeStatus?.ocr.missingModels.join("、") ?? "OCR 模型"}`)}
  </div>
</div>
```

- [x] **Step 4: Add scoped styles**

Add to `App.module.css`:

```css
.runtimeStatus {
  display: grid;
  gap: 7px;
  border-top: 1px solid #e1e7f0;
  padding-top: 8px;
}

.runtimeRow {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
}

.runtimeRow strong {
  color: var(--color-text);
  font-size: 12px;
}

.runtimeMeta {
  overflow-wrap: anywhere;
  color: var(--color-muted);
  font-size: 11px;
  line-height: 1.4;
}
```

- [x] **Step 5: Run UI tests**

Run:

```powershell
Set-Location .\app
npm test
```

Expected: all Vitest tests pass.

### Task 7: README And Verification

**Files:**
- Modify: `README.md`

- [x] **Step 1: Update README setup section**

Add a section named `## 本地模型与密钥配置` after quick start:

```markdown
## 本地模型与密钥配置

DeepSeek API Key 只从本机环境变量读取，仓库只保留 `.env.example` 占位说明，不提交真实密钥。

PowerShell 当前会话示例：

```powershell
$env:DEEPSEEK_API_KEY = "你的本机密钥"
$env:DEEPSEEK_MODEL = "deepseek-v4-flash"
$env:DEEPSEEK_BASE_URL = "https://api.deepseek.com"
```

OCR 模型默认使用 PP-OCRv6 medium。模型文件会下载到 `models/ocr/pp-ocrv6`，该目录已被 Git 忽略。

```powershell
.\scripts\下载OCR模型.ps1 -Tier medium
```

如果希望放到其他目录：

```powershell
.\scripts\下载OCR模型.ps1 -Tier medium -TargetDir "D:\AIModels\Library\ocr\pp-ocrv6"
$env:OCR_MODEL_DIR = "D:\AIModels\Library\ocr\pp-ocrv6"
```

设置面板只显示密钥是否已配置和脱敏片段，不显示完整密钥。
```

- [x] **Step 2: Update feature status**

Move these items from `暂未实现` to `已实现` only after code and tests pass:

```markdown
- DeepSeek 本地安全配置状态读取
- PP-OCRv6 本地模型下载脚本和状态检查
```

Keep these in `暂未实现`:

```markdown
- DeepSeek 问答调用和流式输出
- OCR 推理执行
- 表格深度理解和表格问答
```

- [x] **Step 3: Run full verification**

Run:

```powershell
Set-Location .\app
npm test
npm run build
Set-Location .\src-tauri
cargo fmt -- --check
cargo test
$env:PROTOC = "E:\CodeHome\Library\app\src-tauri\target\protoc\bin\protoc.exe"
cargo check
Set-Location ..\..
git diff --check
```

Expected: all checks pass. If `cargo check` fails only because `protoc` is unavailable, install or point `PROTOC` to a local `protoc.exe` before retrying.

### Task 8: Finish Branch

**Files:**
- No source file edits beyond completed tasks.

- [x] **Step 1: Run secret scan**

Run:

```powershell
rg "s[k]-[A-Za-z0-9]{20,}|DEEPSEEK_API_KEY=.*s[k]-" .
```

Expected: no output.

- [ ] **Step 2: Request reviewer**

Dispatch an independent reviewer subagent with this prompt:

```text
请审查 E:\CodeHome\Library 当前变更，重点检查：
1. 是否满足 DeepSeek 密钥不入库、不展示明文的要求
2. OCR 模型下载目录是否被 Git 忽略
3. Tauri command 是否只暴露脱敏运行状态
4. 前端中文 UI 是否清晰且没有 mock 业务数据回流
5. README 是否没有写入真实密钥
6. 测试和验证是否覆盖核心风险
结论必须是：通过 / 需要修改 / 存在风险但可接受。
```

- [ ] **Step 3: Commit and push after reviewer passes**

Run:

```powershell
git status --short
git add .gitignore .env.example scripts/下载OCR模型.ps1 app/src-tauri/src/runtime.rs app/src-tauri/src/models.rs app/src-tauri/src/commands.rs app/src-tauri/src/lib.rs app/src/types/workbench.ts app/src/lib/tauriClient.ts app/src/hooks/useRuntimeStatus.ts app/src/App.tsx app/src/App.module.css app/src/__tests__/App.test.tsx README.md docs/superpowers/plans/2026-06-22-ai-runtime-and-local-ocr-foundation.md
git commit -m "接入运行配置和本地 OCR 模型状态"
git push origin codex/Library
```

- [ ] **Step 4: Create, merge, and sync PR**

Run:

```powershell
gh pr create --base main --head codex/Library --title "接入运行配置和本地 OCR 模型状态" --body "本 PR 增加 DeepSeek 脱敏运行状态、本地 PP-OCRv6 模型下载脚本和设置面板状态展示。真实 API Key 和模型文件不会进入仓库。"
gh pr merge --merge --delete-branch=false
git fetch origin
git switch main
git pull --ff-only origin main
git switch codex/Library
git merge --ff-only main
git push origin codex/Library
```

Expected: `main` and `codex/Library` both point to the merge commit, and working tree is clean.

## Self-Review

- Spec coverage: The plan covers secret-safe DeepSeek configuration, local OCR model download/status, Chinese UI status, README updates, reviewer, PR, merge, and branch sync.
- Placeholder scan: No `TBD`, `TODO`, or undefined implementation placeholders remain.
- Type consistency: Rust `RuntimeStatus` maps to TypeScript `RuntimeStatus` with camelCase serialization. UI uses the hook names introduced in Task 5.
