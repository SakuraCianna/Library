# Knowledge Base Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first runnable foundation for the personal knowledge base: Tauri desktop shell, Chinese three-column workbench UI, Rust command boundary, SQLite metadata schema, and local LanceDB vector database skeleton.

**Architecture:** The app lives under `app/` and uses Tauri v2 with React/Vite/TypeScript for the UI and Rust for the trusted local core. The frontend renders the confirmed Chinese workbench and calls Tauri commands; Rust owns filesystem-safe state, SQLite metadata, LanceDB local vector storage, permission contracts, and future sidecar orchestration.

**Tech Stack:** Tauri v2, React, Vite, TypeScript, CSS Modules, Rust, rusqlite with SQLite/FTS5, LanceDB Rust crate, Vitest, React Testing Library, Cargo tests.

---

## Scope

This plan implements the foundation only. It does not implement OCR, DeepSeek calls, document parsing, table understanding, backup import/export, or real filesystem scanning. Those are separate subsystem plans after this foundation is running.

## Source References Checked

- Tauri create project docs: https://v2.tauri.app/start/create-project/
- Tauri filesystem plugin docs: https://v2.tauri.app/plugin/file-system/
- Tauri SQL plugin docs, used as reference but not selected for direct frontend DB access: https://v2.tauri.app/plugin/sql/
- LanceDB quickstart: https://docs.lancedb.com/quickstart
- LanceDB Rust crate docs: https://docs.rs/lancedb/latest/lancedb/

## File Structure

```text
E:\CodeHome\Library\
  docs\
    superpowers\
      specs\
        2026-06-21-personal-knowledge-base-design.md
      plans\
        2026-06-21-knowledge-base-foundation.md
  app\
    package.json
    vite.config.ts
    vitest.config.ts
    src\
      App.tsx
      App.module.css
      main.tsx
      styles\
        tokens.css
        global.css
      types\
        workbench.ts
      data\
        mockWorkbench.ts
      lib\
        tauriClient.ts
      hooks\
        useWorkbenchSnapshot.ts
      test\
        setup.ts
      __tests__\
        App.test.tsx
    src-tauri\
      Cargo.toml
      migrations\
        001_foundation.sql
      src\
        lib.rs
        commands.rs
        error.rs
        models.rs
        state.rs
        storage\
          mod.rs
          sqlite.rs
        vector\
          mod.rs
          lancedb_store.rs
```

## Architecture Boundaries

- Frontend may render state and request actions only.
- Rust commands are the only bridge for app state and future write operations.
- SQLite stores business state and FTS5 search tables.
- LanceDB stores local vector records and search-ready metadata.
- Deleted records are first marked in SQLite; vector records must later follow SQLite availability state.
- This phase uses mock workbench data until real folder scanning is implemented.

---

### Task 1: Bootstrap Repository And Tauri App

**Files:**
- Create: `app/` scaffold from Tauri
- Create: `.gitignore`
- Modify: `app/package.json`
- Modify: `app/src-tauri/Cargo.toml`

- [x] **Step 1: Initialize Git on the allowed branch if the workspace is not already a repository**

Run from `E:\CodeHome\Library`:

```powershell
if (git rev-parse --is-inside-work-tree 2>$null) {
  git status --short --branch
} else {
  git init -b codex/Library
  git status --short --branch
}
```

Expected on a new workspace:

```text
Initialized empty Git repository in E:/CodeHome/Library/.git/
## codex/Library
```

- [x] **Step 2: Scaffold a Tauri app**

Run from `E:\CodeHome\Library`:

```powershell
npm create tauri-app@latest app
```

When prompted, choose:

```text
Project name: app
Identifier: com.sakura.personal-knowledge-base
Frontend language: TypeScript / JavaScript
Package manager: npm
Frontend framework: React
Frontend framework option: TypeScript
```

Expected result:

```text
app\package.json exists
app\src\App.tsx exists
app\src-tauri\Cargo.toml exists
```

- [x] **Step 3: Install frontend test dependencies**

Run from `E:\CodeHome\Library\app`:

```powershell
npm install -D vitest jsdom @testing-library/react @testing-library/jest-dom @testing-library/user-event
```

Expected: `package.json` and `package-lock.json` are updated with the dev dependencies.

- [x] **Step 4: Install Rust foundation dependencies**

Run from `E:\CodeHome\Library\app\src-tauri`:

```powershell
cargo add serde --features derive
cargo add serde_json
cargo add thiserror
cargo add uuid --features v4,serde
cargo add time --features serde,formatting,parsing
cargo add rusqlite --features bundled
cargo add walkdir
cargo add blake3
cargo add lancedb
cargo add arrow-array
cargo add arrow-schema
cargo add futures
cargo add tokio --features macros,rt-multi-thread
cargo add tempfile --dev
```

Expected: `Cargo.toml` and `Cargo.lock` include the listed crates.

- [x] **Step 5: Add root `.gitignore`**

Create `E:\CodeHome\Library\.gitignore`:

```gitignore
.superpowers/
app/node_modules/
app/dist/
app/src-tauri/target/
app/.vite/
*.log
*.tmp
```

- [x] **Step 6: Verify scaffold builds**

Run from `E:\CodeHome\Library\app`:

```powershell
npm run build
```

Expected: Vite build succeeds.

Run from `E:\CodeHome\Library\app\src-tauri`:

```powershell
cargo test
```

Expected: Cargo test succeeds or reports zero tests.

- [x] **Step 7: Commit scaffold**

Run from `E:\CodeHome\Library`:

```powershell
git add .gitignore app docs
git commit -m "初始化知识库桌面应用骨架"
```

Expected: commit succeeds on `codex/Library`.

---

### Task 2: Define Shared Workbench Types And Mock Data

**Files:**
- Create: `app/src/types/workbench.ts`
- Create: `app/src/data/mockWorkbench.ts`
- Create: `app/vitest.config.ts`
- Create: `app/src/test/setup.ts`
- Modify: `app/package.json`

- [x] **Step 1: Add Vitest config**

Create `app/vitest.config.ts`:

```ts
import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    globals: true,
  },
});
```

- [x] **Step 2: Add test setup**

Create `app/src/test/setup.ts`:

```ts
import "@testing-library/jest-dom/vitest";
```

- [x] **Step 3: Add test scripts**

Modify `app/package.json` scripts so they include:

```json
{
  "scripts": {
    "dev": "vite",
    "build": "tsc && vite build",
    "preview": "vite preview",
    "tauri": "tauri",
    "test": "vitest run",
    "test:watch": "vitest"
  }
}
```

- [x] **Step 4: Create workbench types**

Create `app/src/types/workbench.ts`:

```ts
export type PermissionMode = "readonly" | "approval" | "full";

export type ChatScope = "current_file" | "current_folder" | "all";

export type ParseStatus = "indexed" | "changed" | "queued" | "failed";

export interface KnowledgeSpace {
  id: string;
  name: string;
  path: string;
  defaultPermission: PermissionMode;
  changedFileCount: number;
  ocrQueueCount: number;
}

export interface KnowledgeFile {
  id: string;
  name: string;
  extension: string;
  status: ParseStatus;
  statusLabel: string;
}

export interface KnowledgeBlockPreview {
  id: string;
  title: string;
  excerpt: string;
  sourceFileName: string;
}

export interface TableInsightPreview {
  id: string;
  title: string;
  description: string;
}

export interface ChatMessage {
  id: string;
  role: "user" | "assistant" | "system";
  content: string;
}

export interface PendingAction {
  id: string;
  label: string;
  requiresApproval: boolean;
}

export interface WorkbenchSnapshot {
  spaces: KnowledgeSpace[];
  activeSpaceId: string;
  activeScope: ChatScope;
  sessionPermission: PermissionMode;
  files: KnowledgeFile[];
  blockPreview: KnowledgeBlockPreview;
  tablePreview: TableInsightPreview;
  messages: ChatMessage[];
  pendingAction: PendingAction | null;
}
```

- [x] **Step 5: Create mock workbench data**

Create `app/src/data/mockWorkbench.ts`:

```ts
import type { WorkbenchSnapshot } from "../types/workbench";

export const mockWorkbench: WorkbenchSnapshot = {
  activeSpaceId: "space-interview",
  activeScope: "current_folder",
  sessionPermission: "approval",
  spaces: [
    {
      id: "space-interview",
      name: "面试",
      path: "D:\\知识库\\面试",
      defaultPermission: "approval",
      changedFileCount: 2,
      ocrQueueCount: 1,
    },
    {
      id: "space-springboot",
      name: "SpringBoot",
      path: "D:\\知识库\\SpringBoot",
      defaultPermission: "readonly",
      changedFileCount: 0,
      ocrQueueCount: 0,
    },
    {
      id: "space-work",
      name: "工作项目A",
      path: "D:\\知识库\\工作项目A",
      defaultPermission: "readonly",
      changedFileCount: 1,
      ocrQueueCount: 0,
    },
  ],
  files: [
    {
      id: "file-java-docx",
      name: "Java面试八股.docx",
      extension: ".docx",
      status: "indexed",
      statusLabel: "已索引",
    },
    {
      id: "file-redis-pdf",
      name: "Redis缓存.pdf",
      extension: ".pdf",
      status: "changed",
      statusLabel: "已变更",
    },
    {
      id: "file-interview-xlsx",
      name: "面试题.xlsx",
      extension: ".xlsx",
      status: "indexed",
      statusLabel: "表格模型就绪",
    },
  ],
  blockPreview: {
    id: "block-redis-cache-penetration",
    title: "知识块预览",
    excerpt:
      "Redis 缓存穿透：请求查询不存在的数据，缓存和数据库都无法命中，导致请求直接打到数据库。",
    sourceFileName: "Redis缓存.pdf",
  },
  tablePreview: {
    id: "table-interview-question-bank",
    title: "表格理解",
    description:
      "识别工作表、表头、字段含义、单位和可问答指标，不做复杂报表仪表盘。",
  },
  messages: [
    {
      id: "msg-user-1",
      role: "user",
      content: "问：Redis 缓存穿透怎么回答面试？",
    },
    {
      id: "msg-assistant-1",
      role: "assistant",
      content: "可以从定义、风险、解决方案和追问点四段回答。我会引用 3 个来源块。",
    },
  ],
  pendingAction: {
    id: "action-flash-card-draft",
    label: "待批准操作：生成复习卡草稿，批准后保存。",
    requiresApproval: true,
  },
};
```

- [x] **Step 6: Run tests**

Run from `E:\CodeHome\Library\app`:

```powershell
npm test
```

Expected: Vitest runs successfully with no tests or passes existing template tests.

- [x] **Step 7: Commit types and mock data**

Run from `E:\CodeHome\Library`:

```powershell
git add app/src/types app/src/data app/src/test app/vitest.config.ts app/package.json app/package-lock.json
git commit -m "添加知识工作台前端数据契约"
```

Expected: commit succeeds.

---

### Task 3: Build The Chinese Three-Column Workbench UI

**Files:**
- Modify: `app/src/App.tsx`
- Create: `app/src/App.module.css`
- Create: `app/src/styles/tokens.css`
- Create: `app/src/styles/global.css`
- Modify: `app/src/main.tsx`
- Create: `app/src/__tests__/App.test.tsx`

- [x] **Step 1: Add CSS design tokens**

Create `app/src/styles/tokens.css`:

```css
:root {
  color-scheme: light;
  --color-bg: #f6f8fb;
  --color-surface: #ffffff;
  --color-surface-subtle: #eef2f7;
  --color-border: #d9dee8;
  --color-border-soft: #e4e9f2;
  --color-text: #182033;
  --color-muted: #637083;
  --color-accent: #245ea8;
  --color-accent-soft: #dbe7f6;
  --color-warning: #9a5b00;
  --color-success: #2d6b35;
  --radius-control: 7px;
  --radius-panel: 8px;
  --space-1: 4px;
  --space-2: 8px;
  --space-3: 12px;
  --space-4: 16px;
  --space-5: 20px;
  --font-ui: ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}
```

- [x] **Step 2: Add global CSS**

Create `app/src/styles/global.css`:

```css
@import "./styles/tokens.css";

* {
  box-sizing: border-box;
}

html,
body,
#root {
  margin: 0;
  min-width: 1024px;
  min-height: 100dvh;
  font-family: var(--font-ui);
  color: var(--color-text);
  background: var(--color-bg);
}

button,
input,
textarea {
  font: inherit;
}

button {
  cursor: pointer;
}

button:disabled {
  cursor: not-allowed;
}
```

- [x] **Step 3: Import global CSS in `main.tsx`**

Replace `app/src/main.tsx` with:

```tsx
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles/global.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
```

- [x] **Step 4: Add workbench CSS module**

Create `app/src/App.module.css`:

```css
.shell {
  display: grid;
  grid-template-columns: 260px minmax(520px, 1fr) 360px;
  min-height: 100dvh;
  background: var(--color-bg);
}

.sidebar {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
  padding: var(--space-4) 14px;
  border-right: 1px solid var(--color-border);
  background: var(--color-surface-subtle);
}

.sidebarHeader,
.agentHeader {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
}

.title {
  font-size: 15px;
  font-weight: 700;
}

.ghostButton,
.plainButton {
  border: 1px solid #c7cfdb;
  border-radius: var(--radius-control);
  background: var(--color-surface);
  color: var(--color-text);
}

.ghostButton {
  padding: 4px 8px;
  font-size: 12px;
}

.searchBox {
  border: 1px solid #d3dae6;
  border-radius: var(--radius-panel);
  background: var(--color-surface);
  padding: 8px 10px;
  font-size: 13px;
  color: #4b5565;
}

.spaceList {
  display: flex;
  flex-direction: column;
  gap: 5px;
}

.spaceItem {
  width: 100%;
  border: 1px solid transparent;
  border-radius: var(--radius-control);
  background: transparent;
  padding: 8px 10px;
  text-align: left;
  color: var(--color-text);
}

.spaceItemActive {
  background: var(--color-accent-soft);
  font-weight: 700;
}

.defaultPermission {
  margin-top: auto;
  border-top: 1px solid #d3dae6;
  padding-top: var(--space-3);
  font-size: 12px;
  color: #606b7b;
}

.defaultPermission strong {
  display: block;
  margin-top: var(--space-1);
  color: var(--color-text);
}

.main {
  display: grid;
  grid-template-rows: auto auto 1fr;
  min-width: 0;
  background: var(--color-surface);
}

.folderHeader {
  display: flex;
  align-items: center;
  border-bottom: 1px solid #e1e6ef;
  padding: 12px 18px;
}

.folderTitleRow {
  display: flex;
  align-items: baseline;
  gap: var(--space-3);
  min-width: 0;
}

.folderName {
  flex: 0 0 auto;
  font-size: 18px;
  font-weight: 700;
}

.folderPath {
  min-width: 0;
  overflow: hidden;
  color: var(--color-muted);
  font-size: 12px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tabs {
  display: flex;
  gap: 10px;
  border-bottom: 1px solid #e1e6ef;
  padding: 10px 18px;
  font-size: 13px;
}

.tabActive,
.tab {
  padding-bottom: 7px;
}

.tabActive {
  border-bottom: 2px solid var(--color-accent);
  font-weight: 700;
}

.tab {
  color: #5b6678;
}

.contentGrid {
  display: grid;
  grid-template-columns: 1.15fr 0.85fr;
  gap: 18px;
  overflow: hidden;
  padding: 12px 18px 18px;
}

.column {
  display: flex;
  flex-direction: column;
  gap: 14px;
  min-width: 0;
}

.statusLine {
  display: flex;
  gap: var(--space-3);
  color: #667085;
  font-size: 12px;
}

.panel {
  border: 1px solid #dce3ee;
  border-radius: var(--radius-panel);
  background: var(--color-surface);
}

.panelPadded {
  padding: 14px;
}

.panelTitle {
  font-size: 14px;
  font-weight: 700;
}

.panelText {
  margin: 10px 0 0;
  color: #4b5565;
  font-size: 13px;
  line-height: 1.6;
}

.fileHeader {
  border-bottom: 1px solid var(--color-border-soft);
  padding: 12px 14px;
  font-size: 14px;
  font-weight: 700;
}

.fileRow {
  display: grid;
  grid-template-columns: 1fr auto;
  gap: var(--space-3);
  border-bottom: 1px solid #eef2f7;
  padding: 11px 14px;
  font-size: 13px;
}

.fileRow:last-child {
  border-bottom: 0;
}

.statusIndexed {
  color: var(--color-success);
}

.statusChanged {
  color: var(--color-warning);
}

.statusQueued {
  color: var(--color-accent);
}

.blockExcerpt {
  margin-top: 10px;
  border: 1px solid #e2e7f0;
  border-radius: var(--radius-control);
  background: #f8fafc;
  padding: 10px;
  font-size: 13px;
  line-height: 1.5;
}

.buttonRow {
  display: flex;
  gap: var(--space-2);
  margin-top: 10px;
}

.plainButton {
  padding: 6px 9px;
  font-size: 12px;
}

.agent {
  display: grid;
  grid-template-rows: auto 1fr auto;
  border-left: 1px solid var(--color-border);
  background: var(--color-bg);
}

.agentTop {
  border-bottom: 1px solid var(--color-border);
  padding: 14px;
}

.permissionPill {
  border: 1px solid #cfd7e5;
  border-radius: 999px;
  background: var(--color-surface);
  padding: 4px 8px;
  font-size: 12px;
}

.scopeGroup {
  display: flex;
  gap: 6px;
  margin-top: var(--space-3);
  font-size: 12px;
}

.scopeActive,
.scope {
  border: 1px solid #d0d7e3;
  border-radius: var(--radius-control);
  padding: 6px 8px;
}

.scopeActive {
  border-color: #b9c7da;
  background: var(--color-accent-soft);
}

.scope {
  background: var(--color-surface);
}

.messages {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: 14px;
  font-size: 13px;
  line-height: 1.5;
}

.messageUser,
.messageAssistant,
.pendingAction {
  max-width: 88%;
  border-radius: var(--radius-panel);
  padding: 10px;
}

.messageUser {
  align-self: flex-start;
  border: 1px solid #dce3ee;
  background: var(--color-surface);
}

.messageAssistant {
  align-self: flex-end;
  border: 1px solid #c9dbf5;
  background: #eaf2ff;
}

.pendingAction {
  border: 1px solid #dce3ee;
  background: var(--color-surface);
  color: #4b5565;
}

.composer {
  border-top: 1px solid var(--color-border);
  padding: var(--space-3);
}

.composerBox {
  border: 1px solid #cfd7e5;
  border-radius: var(--radius-panel);
  background: var(--color-surface);
  padding: 10px;
  color: #687386;
  font-size: 13px;
}
```

- [x] **Step 5: Implement `App.tsx`**

Replace `app/src/App.tsx` with:

```tsx
import { mockWorkbench } from "./data/mockWorkbench";
import type { ChatScope, KnowledgeFile, PermissionMode } from "./types/workbench";
import styles from "./App.module.css";

const permissionLabel: Record<PermissionMode, string> = {
  readonly: "只读",
  approval: "请求批准",
  full: "完全访问",
};

const scopeLabel: Record<ChatScope, string> = {
  current_file: "当前文件",
  current_folder: "当前文件夹",
  all: "全库",
};

const tabs = ["总览", "文件", "知识块", "表格", "回收站"];

function fileStatusClass(file: KnowledgeFile) {
  if (file.status === "changed") return styles.statusChanged;
  if (file.status === "queued") return styles.statusQueued;
  return styles.statusIndexed;
}

export default function App() {
  const snapshot = mockWorkbench;
  const activeSpace = snapshot.spaces.find((space) => space.id === snapshot.activeSpaceId) ?? snapshot.spaces[0];

  return (
    <div className={styles.shell}>
      <aside className={styles.sidebar} aria-label="知识库导航">
        <div className={styles.sidebarHeader}>
          <div className={styles.title}>知识库</div>
          <button className={styles.ghostButton} type="button">新建</button>
        </div>

        <div className={styles.searchBox}>搜索文件、笔记、表格</div>

        <div className={styles.spaceList}>
          {snapshot.spaces.map((space) => (
            <button
              className={`${styles.spaceItem} ${space.id === activeSpace.id ? styles.spaceItemActive : ""}`}
              key={space.id}
              type="button"
            >
              {space.name}
            </button>
          ))}
        </div>

        <div className={styles.defaultPermission}>
          <div>默认权限</div>
          <strong>{permissionLabel[activeSpace.defaultPermission]}</strong>
        </div>
      </aside>

      <main className={styles.main}>
        <header className={styles.folderHeader}>
          <div className={styles.folderTitleRow}>
            <div className={styles.folderName}>{activeSpace.name}</div>
            <div className={styles.folderPath}>{activeSpace.path}</div>
          </div>
        </header>

        <nav className={styles.tabs} aria-label="当前文件夹视图">
          {tabs.map((tab, index) => (
            <span className={index === 0 ? styles.tabActive : styles.tab} key={tab}>
              {tab}
            </span>
          ))}
        </nav>

        <section className={styles.contentGrid}>
          <div className={styles.column}>
            <div className={styles.statusLine}>
              <span>变更文件 {activeSpace.changedFileCount}</span>
              <span>文字识别队列 {activeSpace.ocrQueueCount}</span>
              <span>默认权限：{permissionLabel[activeSpace.defaultPermission]}</span>
            </div>

            <section className={`${styles.panel} ${styles.panelPadded}`}>
              <div className={styles.panelTitle}>文件夹总览 README.md</div>
              <p className={styles.panelText}>
                自动生成文件夹总览，用户可以自由修改。重新解析时只产生更新建议，不覆盖手写内容。
              </p>
            </section>

            <section className={styles.panel}>
              <div className={styles.fileHeader}>当前文件夹文件</div>
              {snapshot.files.map((file) => (
                <div className={styles.fileRow} key={file.id}>
                  <span>{file.name}</span>
                  <span className={fileStatusClass(file)}>{file.statusLabel}</span>
                </div>
              ))}
            </section>
          </div>

          <div className={styles.column}>
            <section className={`${styles.panel} ${styles.panelPadded}`}>
              <div className={styles.panelTitle}>{snapshot.blockPreview.title}</div>
              <div className={styles.blockExcerpt}>{snapshot.blockPreview.excerpt}</div>
              <div className={styles.buttonRow}>
                <button className={styles.plainButton} type="button">查看来源</button>
                <button className={styles.plainButton} type="button">移入回收站</button>
              </div>
            </section>

            <section className={`${styles.panel} ${styles.panelPadded}`}>
              <div className={styles.panelTitle}>{snapshot.tablePreview.title}</div>
              <p className={styles.panelText}>{snapshot.tablePreview.description}</p>
            </section>
          </div>
        </section>
      </main>

      <aside className={styles.agent} aria-label="智能助手">
        <div className={styles.agentTop}>
          <div className={styles.agentHeader}>
            <div className={styles.title}>智能助手</div>
            <span className={styles.permissionPill}>{permissionLabel[snapshot.sessionPermission]}</span>
          </div>
          <div className={styles.scopeGroup}>
            {(["current_folder", "current_file", "all"] as ChatScope[]).map((scope) => (
              <span className={scope === snapshot.activeScope ? styles.scopeActive : styles.scope} key={scope}>
                {scopeLabel[scope]}
              </span>
            ))}
          </div>
        </div>

        <div className={styles.messages}>
          {snapshot.messages.map((message) => (
            <div
              className={message.role === "user" ? styles.messageUser : styles.messageAssistant}
              key={message.id}
            >
              {message.content}
            </div>
          ))}
          {snapshot.pendingAction ? (
            <div className={styles.pendingAction}>{snapshot.pendingAction.label}</div>
          ) : null}
        </div>

        <div className={styles.composer}>
          <div className={styles.composerBox}>询问当前文件夹...</div>
        </div>
      </aside>
    </div>
  );
}
```

- [x] **Step 6: Add UI test**

Create `app/src/__tests__/App.test.tsx`:

```tsx
import { render, screen } from "@testing-library/react";
import App from "../App";

describe("App", () => {
  it("renders the confirmed Chinese knowledge workbench", () => {
    render(<App />);

    expect(screen.getByText("知识库")).toBeInTheDocument();
    expect(screen.getAllByText("面试").length).toBeGreaterThan(0);
    expect(screen.getByText("D:\\知识库\\面试")).toBeInTheDocument();
    expect(screen.getByText("文件夹总览 README.md")).toBeInTheDocument();
    expect(screen.getByText("智能助手")).toBeInTheDocument();
    expect(screen.getByText("询问当前文件夹...")).toBeInTheDocument();
  });
});
```

- [x] **Step 7: Run frontend checks**

Run from `E:\CodeHome\Library\app`:

```powershell
npm test
npm run build
```

Expected: tests pass and Vite build succeeds.

- [x] **Step 8: Commit UI**

Run from `E:\CodeHome\Library`:

```powershell
git add app/src app/package.json app/package-lock.json app/vitest.config.ts
git commit -m "实现中文三栏知识工作台界面"
```

Expected: commit succeeds.

---

### Task 4: Add Rust Domain Models And Tauri Commands

**Files:**
- Create: `app/src-tauri/src/models.rs`
- Create: `app/src-tauri/src/error.rs`
- Create: `app/src-tauri/src/state.rs`
- Create: `app/src-tauri/src/commands.rs`
- Modify: `app/src-tauri/src/lib.rs`

- [x] **Step 1: Create Rust models**

Create `app/src-tauri/src/models.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    Readonly,
    Approval,
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatScope {
    CurrentFile,
    CurrentFolder,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeSpace {
    pub id: String,
    pub name: String,
    pub path: String,
    pub default_permission: PermissionMode,
    pub changed_file_count: u32,
    pub ocr_queue_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbenchSnapshot {
    pub spaces: Vec<KnowledgeSpace>,
    pub active_space_id: String,
    pub active_scope: ChatScope,
    pub session_permission: PermissionMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequest {
    pub requested: PermissionMode,
}

pub fn can_temporarily_escalate(folder_default: &PermissionMode, requested: &PermissionMode) -> bool {
    matches!(
        (folder_default, requested),
        (PermissionMode::Readonly, PermissionMode::Approval)
            | (PermissionMode::Approval, PermissionMode::Approval)
            | (PermissionMode::Full, PermissionMode::Approval)
            | (PermissionMode::Full, PermissionMode::Full)
    )
}
```

- [x] **Step 2: Add Rust error type**

Create `app/src-tauri/src/error.rs`:

```rust
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("权限不足：{0}")]
    PermissionDenied(String),
    #[error("本地存储错误：{0}")]
    Storage(String),
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub message: String,
}

impl From<AppError> for ErrorResponse {
    fn from(value: AppError) -> Self {
        Self {
            message: value.to_string(),
        }
    }
}
```

- [x] **Step 3: Add app state**

Create `app/src-tauri/src/state.rs`:

```rust
use std::sync::Mutex;

use crate::models::{ChatScope, KnowledgeSpace, PermissionMode, WorkbenchSnapshot};

pub struct AppState {
    snapshot: Mutex<WorkbenchSnapshot>,
}

impl AppState {
    pub fn new_with_mock_data() -> Self {
        Self {
            snapshot: Mutex::new(WorkbenchSnapshot {
                active_space_id: "space-interview".to_string(),
                active_scope: ChatScope::CurrentFolder,
                session_permission: PermissionMode::Approval,
                spaces: vec![
                    KnowledgeSpace {
                        id: "space-interview".to_string(),
                        name: "面试".to_string(),
                        path: "D:\\知识库\\面试".to_string(),
                        default_permission: PermissionMode::Approval,
                        changed_file_count: 2,
                        ocr_queue_count: 1,
                    },
                    KnowledgeSpace {
                        id: "space-springboot".to_string(),
                        name: "SpringBoot".to_string(),
                        path: "D:\\知识库\\SpringBoot".to_string(),
                        default_permission: PermissionMode::Readonly,
                        changed_file_count: 0,
                        ocr_queue_count: 0,
                    },
                ],
            }),
        }
    }

    pub fn snapshot(&self) -> WorkbenchSnapshot {
        self.snapshot
            .lock()
            .expect("workbench snapshot mutex poisoned")
            .clone()
    }

    pub fn set_session_permission(&self, permission: PermissionMode) {
        self.snapshot
            .lock()
            .expect("workbench snapshot mutex poisoned")
            .session_permission = permission;
    }
}
```

- [x] **Step 4: Add commands**

Create `app/src-tauri/src/commands.rs`:

```rust
use tauri::State;

use crate::error::{AppError, ErrorResponse};
use crate::models::{can_temporarily_escalate, PermissionRequest, WorkbenchSnapshot};
use crate::state::AppState;

#[tauri::command]
pub fn get_workbench_snapshot(state: State<'_, AppState>) -> WorkbenchSnapshot {
    state.snapshot()
}

#[tauri::command]
pub fn set_session_permission(
    state: State<'_, AppState>,
    request: PermissionRequest,
) -> Result<WorkbenchSnapshot, ErrorResponse> {
    let snapshot = state.snapshot();
    let active_space = snapshot
        .spaces
        .iter()
        .find(|space| space.id == snapshot.active_space_id)
        .ok_or_else(|| AppError::Storage("找不到当前知识库".to_string()))?;

    if !can_temporarily_escalate(&active_space.default_permission, &request.requested) {
        return Err(AppError::PermissionDenied("当前文件夹默认权限不允许这样临时升权".to_string()).into());
    }

    state.set_session_permission(request.requested);
    Ok(state.snapshot())
}
```

- [x] **Step 5: Wire commands into `lib.rs`**

Replace `app/src-tauri/src/lib.rs` with:

```rust
mod commands;
mod error;
mod models;
mod state;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::new_with_mock_data())
        .invoke_handler(tauri::generate_handler![
            commands::get_workbench_snapshot,
            commands::set_session_permission
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [x] **Step 6: Add Rust unit tests**

Append to `app/src-tauri/src/models.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::{can_temporarily_escalate, PermissionMode};

    #[test]
    fn readonly_folder_can_temporarily_escalate_to_approval() {
        assert!(can_temporarily_escalate(
            &PermissionMode::Readonly,
            &PermissionMode::Approval
        ));
    }

    #[test]
    fn readonly_folder_cannot_temporarily_escalate_to_full() {
        assert!(!can_temporarily_escalate(
            &PermissionMode::Readonly,
            &PermissionMode::Full
        ));
    }
}
```

- [x] **Step 7: Run Rust checks**

Run from `E:\CodeHome\Library\app\src-tauri`:

```powershell
cargo fmt
cargo test
```

Expected: formatting succeeds and tests pass.

- [x] **Step 8: Commit Rust command boundary**

Run from `E:\CodeHome\Library`:

```powershell
git add app/src-tauri
git commit -m "建立 Rust 权限模型和工作台命令边界"
```

Expected: commit succeeds.

---

### Task 5: Add SQLite Metadata Schema And Repository

**Files:**
- Create: `app/src-tauri/migrations/001_foundation.sql`
- Create: `app/src-tauri/src/storage/mod.rs`
- Create: `app/src-tauri/src/storage/sqlite.rs`
- Modify: `app/src-tauri/src/lib.rs`

- [x] **Step 1: Add SQLite schema**

Create `app/src-tauri/migrations/001_foundation.sql`:

```sql
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS knowledge_spaces (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  root_path TEXT NOT NULL UNIQUE,
  default_permission TEXT NOT NULL CHECK (default_permission IN ('readonly', 'approval', 'full')),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT
);

CREATE TABLE IF NOT EXISTS files (
  id TEXT PRIMARY KEY,
  space_id TEXT NOT NULL REFERENCES knowledge_spaces(id),
  relative_path TEXT NOT NULL,
  extension TEXT NOT NULL,
  content_hash TEXT,
  modified_at TEXT,
  parse_status TEXT NOT NULL CHECK (parse_status IN ('indexed', 'changed', 'queued', 'failed')),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  UNIQUE(space_id, relative_path)
);

CREATE TABLE IF NOT EXISTS markdown_notes (
  id TEXT PRIMARY KEY,
  file_id TEXT REFERENCES files(id),
  space_id TEXT NOT NULL REFERENCES knowledge_spaces(id),
  relative_path TEXT NOT NULL,
  user_editable INTEGER NOT NULL DEFAULT 1,
  last_generated_hash TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT
);

CREATE TABLE IF NOT EXISTS knowledge_blocks (
  id TEXT PRIMARY KEY,
  space_id TEXT NOT NULL REFERENCES knowledge_spaces(id),
  file_id TEXT REFERENCES files(id),
  note_id TEXT REFERENCES markdown_notes(id),
  title TEXT NOT NULL,
  body TEXT NOT NULL,
  source_kind TEXT NOT NULL CHECK (source_kind IN ('original_file', 'markdown_note', 'table')),
  source_locator TEXT NOT NULL,
  searchable INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT
);

CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_blocks_fts USING fts5(
  title,
  body,
  content='knowledge_blocks',
  content_rowid='rowid'
);

CREATE TABLE IF NOT EXISTS parse_jobs (
  id TEXT PRIMARY KEY,
  space_id TEXT NOT NULL REFERENCES knowledge_spaces(id),
  file_id TEXT REFERENCES files(id),
  job_type TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
  error_message TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS trash_entries (
  id TEXT PRIMARY KEY,
  space_id TEXT NOT NULL REFERENCES knowledge_spaces(id),
  entity_kind TEXT NOT NULL CHECK (entity_kind IN ('file', 'markdown_note', 'knowledge_block')),
  entity_id TEXT NOT NULL,
  display_name TEXT NOT NULL,
  original_locator TEXT NOT NULL,
  deleted_at TEXT NOT NULL,
  restored_at TEXT
);
```

- [x] **Step 2: Add storage module**

Create `app/src-tauri/src/storage/mod.rs`:

```rust
pub mod sqlite;
```

- [x] **Step 3: Add SQLite repository**

Create `app/src-tauri/src/storage/sqlite.rs`:

```rust
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
        self.connection.execute_batch(include_str!("../../migrations/001_foundation.sql"))
    }

    pub fn create_knowledge_space(&self, name: &str, root_path: &str, default_permission: &str) -> rusqlite::Result<String> {
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
        self.connection
            .query_row("SELECT COUNT(*) FROM knowledge_spaces WHERE deleted_at IS NULL", [], |row| row.get::<_, u32>(0))
    }
}

#[cfg(test)]
mod tests {
    use super::SqliteStore;

    #[test]
    fn creates_knowledge_space_in_local_sqlite() {
        let store = SqliteStore::open_in_memory().expect("in-memory sqlite opens");
        let id = store
            .create_knowledge_space("面试", "D:\\知识库\\面试", "approval")
            .expect("space is inserted");

        assert!(!id.is_empty());
        assert_eq!(store.count_knowledge_spaces().unwrap(), 1);
    }
}
```

- [x] **Step 4: Export storage module in `lib.rs`**

Modify `app/src-tauri/src/lib.rs` module declarations:

```rust
mod commands;
mod error;
mod models;
mod state;
mod storage;
```

- [x] **Step 5: Run SQLite tests**

Run from `E:\CodeHome\Library\app\src-tauri`:

```powershell
cargo fmt
cargo test storage::sqlite
```

Expected: SQLite test passes and confirms schema can create a knowledge space.

- [x] **Step 6: Commit SQLite foundation**

Run from `E:\CodeHome\Library`:

```powershell
git add app/src-tauri
git commit -m "添加本地 SQLite 元数据骨架"
```

Expected: commit succeeds.

---

### Task 6: Add Local LanceDB Vector Store Skeleton

**Files:**
- Create: `app/src-tauri/src/vector/mod.rs`
- Create: `app/src-tauri/src/vector/lancedb_store.rs`
- Modify: `app/src-tauri/src/lib.rs`

- [x] **Step 1: Add vector module**

Create `app/src-tauri/src/vector/mod.rs`:

```rust
pub mod lancedb_store;
```

- [x] **Step 2: Add LanceDB local connection wrapper**

Create `app/src-tauri/src/vector/lancedb_store.rs`:

```rust
use std::path::{Path, PathBuf};

pub struct LanceVectorStore {
    path: PathBuf,
}

impl LanceVectorStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn connect(&self) -> lancedb::Result<lancedb::Connection> {
        lancedb::connect(self.path.to_string_lossy().as_ref())
            .execute()
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::LanceVectorStore;

    #[tokio::test]
    async fn connects_to_local_lancedb_path() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let store = LanceVectorStore::new(temp_dir.path().join("vectors.lance"));
        let connection = store.connect().await.expect("local LanceDB connection opens");
        drop(connection);
        assert!(store.path().to_string_lossy().contains("vectors.lance"));
    }
}
```

- [x] **Step 3: Export vector module in `lib.rs`**

Modify `app/src-tauri/src/lib.rs` module declarations:

```rust
mod commands;
mod error;
mod models;
mod state;
mod storage;
mod vector;
```

- [x] **Step 4: Run LanceDB test**

Run from `E:\CodeHome\Library\app\src-tauri`:

```powershell
cargo fmt
cargo test vector::lancedb_store
```

Expected: local LanceDB connection test passes without using a cloud URI.

- [x] **Step 5: Commit LanceDB foundation**

Run from `E:\CodeHome\Library`:

```powershell
git add app/src-tauri
git commit -m "添加本地 LanceDB 向量库骨架"
```

Expected: commit succeeds.

---

### Task 7: Wire Frontend To Tauri Snapshot Command With Mock Fallback

**Files:**
- Create: `app/src/lib/tauriClient.ts`
- Create: `app/src/hooks/useWorkbenchSnapshot.ts`
- Modify: `app/src/App.tsx`
- Create: `app/src/hooks/useWorkbenchSnapshot.test.ts`

- [x] **Step 1: Add Tauri client wrapper**

Create `app/src/lib/tauriClient.ts`:

```ts
import { invoke } from "@tauri-apps/api/core";
import { mockWorkbench } from "../data/mockWorkbench";
import type { WorkbenchSnapshot } from "../types/workbench";

export async function getWorkbenchSnapshot(): Promise<WorkbenchSnapshot> {
  if (!("__TAURI_INTERNALS__" in window)) {
    return mockWorkbench;
  }

  return invoke<WorkbenchSnapshot>("get_workbench_snapshot");
}
```

- [x] **Step 2: Add hook**

Create `app/src/hooks/useWorkbenchSnapshot.ts`:

```ts
import { useEffect, useState } from "react";
import { mockWorkbench } from "../data/mockWorkbench";
import { getWorkbenchSnapshot } from "../lib/tauriClient";
import type { WorkbenchSnapshot } from "../types/workbench";

interface WorkbenchState {
  snapshot: WorkbenchSnapshot;
  loading: boolean;
  error: string | null;
}

export function useWorkbenchSnapshot(): WorkbenchState {
  const [state, setState] = useState<WorkbenchState>({
    snapshot: mockWorkbench,
    loading: true,
    error: null,
  });

  useEffect(() => {
    let active = true;

    getWorkbenchSnapshot()
      .then((snapshot) => {
        if (active) {
          setState({ snapshot, loading: false, error: null });
        }
      })
      .catch((error: unknown) => {
        if (active) {
          setState({
            snapshot: mockWorkbench,
            loading: false,
            error: error instanceof Error ? error.message : "读取工作台状态失败",
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

- [x] **Step 3: Use hook in `App.tsx`**

In `app/src/App.tsx`, replace:

```tsx
const snapshot = mockWorkbench;
```

with:

```tsx
const { snapshot, error } = useWorkbenchSnapshot();
```

Also add the import:

```tsx
import { useWorkbenchSnapshot } from "./hooks/useWorkbenchSnapshot";
```

Render the error near the status line:

```tsx
{error ? <span>状态读取失败，正在显示本地示例</span> : null}
```

- [x] **Step 4: Update App import cleanup**

Remove this import from `app/src/App.tsx`:

```tsx
import { mockWorkbench } from "./data/mockWorkbench";
```

- [x] **Step 5: Run frontend checks**

Run from `E:\CodeHome\Library\app`:

```powershell
npm test
npm run build
```

Expected: UI tests still pass and build succeeds.

- [x] **Step 6: Run Tauri command check**

Run from `E:\CodeHome\Library\app\src-tauri`:

```powershell
cargo test
```

Expected: Rust tests pass.

- [x] **Step 7: Commit frontend command wiring**

Run from `E:\CodeHome\Library`:

```powershell
git add app/src app/src-tauri
git commit -m "连接前端工作台与 Tauri 状态命令"
```

Expected: commit succeeds.

---

### Task 8: Add Foundation Documentation And Final Verification

**Files:**
- Create: `app/README.md`
- Modify: `docs/superpowers/specs/2026-06-21-personal-knowledge-base-design.md`

- [x] **Step 1: Create app README**

Create `app/README.md`:

```md
# 个人知识库桌面应用

这是个人知识库桌面应用的第一阶段工程骨架。

## 当前已实现

- Tauri v2 桌面应用骨架
- React/Vite/TypeScript 前端
- 中文三栏知识工作台界面
- Rust Tauri 命令边界
- SQLite 本地元数据 schema
- LanceDB 本地向量库连接骨架

## 本地开发

安装依赖：

```powershell
npm install
```

运行前端检查：

```powershell
npm test
npm run build
```

运行 Rust 检查：

```powershell
Set-Location .\src-tauri
cargo fmt --check
cargo test
```

启动桌面应用：

```powershell
npm run tauri dev
```

## 架构边界

前端只负责展示和请求操作。所有文件、数据库、权限和未来高风险操作都必须经过 Rust 核心。SQLite 和 LanceDB 都是本地数据库，不使用云数据库或云端向量库。
```

- [x] **Step 2: Add foundation milestone note to the spec**

Append to `docs/superpowers/specs/2026-06-21-personal-knowledge-base-design.md`:

```md

## 16. 第一阶段实现计划

第一阶段实现计划见 `docs/superpowers/plans/2026-06-21-knowledge-base-foundation.md`。

第一阶段只覆盖桌面壳、中文三栏界面、Rust 命令边界、SQLite 元数据骨架和 LanceDB 本地向量库骨架。OCR、DeepSeek、文档解析、表格问答、回收站和备份导入导出将拆成后续独立计划。
```

- [x] **Step 3: Run full verification**

Run from `E:\CodeHome\Library\app`:

```powershell
npm test
npm run build
```

Expected: frontend tests and build pass.

Run from `E:\CodeHome\Library\app\src-tauri`:

```powershell
cargo fmt --check
cargo test
```

Expected: Rust formatting check and tests pass.

- [x] **Step 4: Review changed files**

Run from `E:\CodeHome\Library`:

```powershell
git status --short
```

Expected: only planned `app/`, `.gitignore`, `docs/` files are changed.

- [x] **Step 5: Commit foundation docs**

Run from `E:\CodeHome\Library`:

```powershell
git add app/README.md docs/superpowers/specs/2026-06-21-personal-knowledge-base-design.md docs/superpowers/plans/2026-06-21-knowledge-base-foundation.md
git commit -m "补充第一阶段实现计划和开发说明"
```

Expected: commit succeeds.

---

## Self-Review Checklist

- Spec coverage: This plan covers the first implementation slice from the design spec: desktop shell, Chinese workbench UI, Rust command boundary, local SQLite metadata skeleton, and local LanceDB vector skeleton.
- Explicitly deferred: OCR, DeepSeek, document parsing, table understanding, permission execution flows, trash behavior, and backup import/export are intentionally deferred into later plans.
- Placeholder scan: No task contains unfinished markers or unspecified implementation.
- Type consistency: Frontend `PermissionMode`, `ChatScope`, and Rust `PermissionMode`, `ChatScope` use the same snake_case serialized values.
- Windows commands: All commands use PowerShell-compatible syntax.
