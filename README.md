# 个人知识库桌面应用

一个本地优先的 Windows 桌面端个人知识库应用。目标是让用户把 Word、PDF、XLSX、Markdown 等资料放入真实文件夹后，由应用建立本地元数据、全文检索、向量索引和智能助手工作台，逐步形成长期个人知识资产。

当前仓库处于第一阶段工程骨架：已经完成桌面壳、中文三栏界面、Rust 命令边界、本地 SQLite 元数据骨架、本地 LanceDB 向量库骨架，以及真实文件夹登记和文件元数据扫描入库。

## 功能状态

### 已实现

- Windows 桌面应用骨架，基于 Tauri v2
- React + Vite + TypeScript 前端工程
- 全中文三栏知识工作台界面
- 文件夹式知识空间和前端类型契约
- 通过桌面文件夹选择器新建真实知识库
- 扫描 `.pdf`、`.docx`、`.xlsx`、`.md`、`.txt` 文件元数据
- 增量识别新增、变更和删除文件
- 对 `.md`、`.txt` 做真实文本抽取
- 对 `.docx`、`.xlsx` 做 Office ZIP XML 轻量文本抽取
- 对文本型 `.pdf` 做 MVP 轻量文本抽取
- 将解析结果写入统一的 `knowledge_blocks` 结构化数据层
- 为知识块建立 SQLite FTS5 全文索引和摘要预览
- 侧边栏智能助手可基于本地索引检索并回答
- DeepSeek Chat Completions 调用边界，失败或未配置时使用本地检索兜底回答
- Rust Tauri 命令边界
- 文件夹默认权限与当前会话权限分离
- 左下角文件夹默认权限下拉设置
- 右侧当前会话权限下拉设置
- SQLite 本地元数据 schema 和 repository 骨架
- SQLite FTS5 全文检索基础表和触发器
- LanceDB 本地向量库连接骨架
- LanceDB 本地路径边界校验，拒绝远程或云端 URI
- 浏览器环境空状态 fallback，Tauri 环境读取真实本地工作台状态
- Iconify 图标组件和本地 lucide 单图标依赖
- GitHub Actions CI 工作流
- Windows 桌面端 Release 构建工作流
- DeepSeek 本地安全配置状态读取
- PP-OCRv6 本地模型下载脚本和状态检查
- 本地 OCR sidecar JSON 协议骨架
- SQLite 解析队列入队、取消和列表查询骨架
- 前端解析队列状态展示
- 前端和 Rust 单元测试

### 权限说明

默认权限是某个知识库文件夹长期保存的 Agent 操作边界，保存在本地 SQLite 中。它决定这个文件夹默认允许助手做到哪一步。

当前会话权限只影响右侧智能助手的本次聊天，不会写回文件夹默认配置。会话权限不能超过文件夹默认权限允许的上限；高风险操作后续仍会要求二次确认。

### 暂未实现

- 扫描版 PDF 和图片 OCR 推理执行
- OCR sidecar 真实推理依赖安装与调用
- 高保真 PDF 版面解析
- 表格深度理解和表格问答
- DeepSeek 流式输出
- Python 文档解析 Sidecar
- 回收站执行流
- 备份、导出、导入
- Windows 代码签名、正式发布验收和自动更新

## 技术栈

- 桌面端：Tauri v2
- 前端：React、Vite、TypeScript
- 样式：CSS Modules、CSS 变量、Iconify 图标
- Rust 核心：Tauri commands、状态管理、权限边界
- 元数据数据库：SQLite、FTS5
- 向量数据库：LanceDB 本地 embedded 模式
- 测试：Vitest、React Testing Library、Cargo test

## 快速启动

首次运行前需要准备：

- Windows 11
- PowerShell 7
- Node.js，版本满足 `app/package.json` 中的 `engines`
- Rust stable toolchain
- Protocol Buffers compiler，即 `protoc.exe`

安装前端依赖：

```powershell
Set-Location .\app
npm install
```

之后可以在项目根目录双击：

```text
快速启动.bat
```

也可以手动启动：

```powershell
Set-Location .\app
npm run tauri dev
```

如果控制台出现 `ESC[32m`、`ESC[1m` 这类文字，它们是 Vite、Tauri 或 Cargo 输出的 ANSI 颜色控制码被当前终端当成普通文本显示，不代表编译失败。建议使用 Windows Terminal 或 PowerShell 7 运行。

如果 Rust 构建提示找不到 `protoc`，需要安装 Protocol Buffers compiler，并确保 `protoc.exe` 在 `PATH` 中。也可以在当前 PowerShell 会话临时指定：

```powershell
$env:PROTOC = "C:\path\to\protoc.exe"
```

## 本地模型与密钥配置

DeepSeek API Key 只从本机环境变量读取，仓库只保留 `.env.example` 占位说明，不提交真实密钥。设置面板只显示密钥是否已配置和脱敏片段，不显示完整密钥。

PowerShell 当前会话示例：

```powershell
$env:DEEPSEEK_API_KEY = "你的本机密钥"
$env:DEEPSEEK_MODEL = "deepseek-v4-flash"
$env:DEEPSEEK_BASE_URL = "https://api.deepseek.com"
```

OCR 模型默认使用 PP-OCRv6 medium。模型文件默认下载到 `models/ocr/pp-ocrv6`，该目录已被 Git 忽略。

```powershell
.\scripts\下载OCR模型.ps1 -Tier medium
$env:OCR_MODEL_DIR = "E:\CodeHome\Library\models\ocr\pp-ocrv6"
```

如果希望放到其他目录：

```powershell
.\scripts\下载OCR模型.ps1 -Tier medium -TargetDir "D:\AIModels\Library\ocr\pp-ocrv6"
$env:OCR_MODEL_DIR = "D:\AIModels\Library\ocr\pp-ocrv6"
```

当前 OCR 阶段只完成模型下载和状态检查。OCR 推理执行、扫描件解析和表格深度理解会在后续解析模块接入。

## MVP 最短链路

当前可以跑通：

```text
新建真实知识库文件夹 -> 扫描 -> 建索引/摘要 -> 在右侧智能助手提问
```

建索引/摘要会把扫描到的 `.md`、`.txt`、`.docx`、`.xlsx` 和文本型 `.pdf` 转成统一知识块并写入本地 SQLite。助手回答会先检索本地知识块；如果本机配置了 DeepSeek API Key，会尝试使用 `deepseek-v4-flash` 生成更自然的回答，失败时自动回退到本地检索结果。

当前 PDF 解析是 MVP 轻量能力，适合可抽取文本的 PDF。扫描版 PDF、复杂版面和图片文字需要后续 OCR 管线。

## 常用命令

前端测试：

```powershell
Set-Location .\app
npm test
```

前端构建：

```powershell
Set-Location .\app
npm run build
```

Rust 检查：

```powershell
Set-Location .\app\src-tauri
cargo fmt -- --check
cargo test
cargo check
```

## CI/CD

仓库提供两条 GitHub Actions 工作流：

- `CI`：PR、`main`、`codex/**` 分支推送和手动触发时运行，检查前端测试、前端构建、OCR sidecar 测试、Rust 格式、Rust 测试和 Rust 编译
- `Release`：推送 `v*.*.*` tag 或手动触发时运行，构建 Windows Tauri 安装产物并上传 artifact；tag 触发时会创建 GitHub Release 草稿

详细说明见 [CI/CD 工作流](docs/ci-cd.md)。

## 目录结构

```text
.
├── .github
│   └── workflows
├── app
│   ├── src
│   │   ├── data
│   │   ├── hooks
│   │   ├── lib
│   │   ├── styles
│   │   └── types
│   ├── src-tauri
│   │   ├── migrations
│   │   └── src
│   │       ├── scanner
│   │       ├── storage
│   │       └── vector
│   └── package.json
├── docs
│   ├── ci-cd.md
│   └── superpowers
├── scripts
│   └── 下载OCR模型.ps1
├── sidecars
│   └── ocr
├── README.md
└── 快速启动.bat
```

## 架构边界

前端只负责展示和发起明确请求。文件系统访问、数据库读写、权限校验、未来高风险操作确认，都应经过 Rust 核心。

SQLite 和 LanceDB 都是本地数据库，不使用云数据库或云端向量库。云端模型调用后续只用于推理，不作为个人知识资产的长期存储。

## 开发约定

- 界面文案默认使用中文
- 业务样式优先放在组件级 CSS Module
- 全局 CSS 只放 reset、字体、变量和基础样式
- 不把密钥、token、cookie、私有证书写入仓库
- 未验证通过的能力不要写成已完成
- 高风险操作需要展示影响范围并二次确认

## 当前验证

当前第一阶段已通过以下检查：

- `python -m pytest`
- `npm test`
- `npm run build`
- `cargo fmt -- --check`
- `cargo test`
- `cargo check`

说明：`cargo check` 需要当前环境能找到 `protoc.exe`。如果未配置 `PATH` 或 `PROTOC`，会在 LanceDB 依赖构建阶段失败。

CI 工作流会在 Windows runner 中安装 `protoc` 并设置 `PROTOC`，本地运行仍需要开发者自行准备。

当前扫描阶段会同步遍历支持格式文件并计算内容指纹。超大目录后续需要接入后台任务、进度、取消和文件大小限制。

当前解析阶段是同步 MVP 链路，适合先验证单文件和小文件夹。后续需要改成后台任务队列、进度展示、取消和重试。
