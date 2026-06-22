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
- 扫描 `.pdf`、`.docx`、`.xlsx`、`.md`、`.txt` 和常见图片文件元数据
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
- 桌面应用内 OCR 环境自检入口
- 本地 OCR sidecar JSON 协议和 PaddleOCR 真实推理入口
- 图片文件可进入本地 OCR 队列并写入统一知识块
- SQLite 解析队列入队、后台执行、取消、进度和列表查询骨架
- 扫描文件夹接入后台任务队列、粗粒度进度和取消
- 扫描后自动为待解析文档创建后台解析任务
- 扫描后自动为图片文件创建本地 OCR 任务
- 普通文档解析后台 worker，可逐个处理 `.pdf`、`.docx`、`.xlsx`、`.md`、`.txt`
- 普通文档解析失败后可通过再次建索引/摘要手动重试
- 前端解析队列状态展示和运行中任务轮询刷新
- 前端可启动后台 OCR worker、刷新进度并取消排队或运行中的 OCR 任务
- 后台扫描、文档解析和 OCR worker 会通过 Tauri 事件通知前端刷新队列状态，并保留轮询兜底
- OCR sidecar 可输出页/段进度流，PDF 按页更新队列进度，图片按单段更新队列进度
- 前端和 Rust 单元测试

### 权限说明

默认权限是某个知识库文件夹长期保存的 Agent 操作边界，保存在本地 SQLite 中。它决定这个文件夹默认允许助手做到哪一步。

当前会话权限只影响右侧智能助手的本次聊天，不会写回文件夹默认配置。会话权限不能超过文件夹默认权限允许的上限；高风险操作后续仍会要求二次确认。

### 暂未实现

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

OCR 模型默认使用 PP-OCRv6 medium。先准备项目本地 Python 环境，避免把下载依赖安装到全局 Python。模型文件默认下载到 `models/ocr/pp-ocrv6`，该目录已被 Git 忽略。

```powershell
python -m venv .venv
.\.venv\Scripts\python.exe -m pip install -r .\sidecars\ocr\requirements.txt
.\scripts\下载OCR模型.ps1 -Tier medium -PythonPath .\.venv\Scripts\python.exe -SkipExisting
.\scripts\检查OCR环境.ps1 -Tier medium
$env:OCR_MODEL_DIR = "E:\CodeHome\Library\models\ocr\pp-ocrv6"
$env:OCR_MAX_PDF_PAGES = "12"
```

如果希望放到其他目录：

```powershell
.\scripts\下载OCR模型.ps1 -Tier medium -PythonPath .\.venv\Scripts\python.exe -TargetDir "D:\AIModels\Library\ocr\pp-ocrv6"
$env:OCR_MODEL_DIR = "D:\AIModels\Library\ocr\pp-ocrv6"
```

如果已经安装依赖和下载模型，可随时单独运行自检：

```powershell
.\scripts\检查OCR环境.ps1 -Tier medium
```

桌面应用左侧默认权限设置面板也提供 OCR 自检入口，会检查模型文件、sidecar、`pypdf`、`paddleocr` 和 `paddlepaddle`。

桌面构建会把 `sidecars/ocr/ocr_sidecar.py`、`check_ocr_environment.py` 和 `requirements.txt` 作为 Tauri resource 打包。开发态优先使用仓库根目录下的 `.venv\Scripts\python.exe`，打包后可通过本机环境变量显式指定：

```powershell
$env:OCR_PYTHON_PATH = "E:\CodeHome\Library\.venv\Scripts\python.exe"
$env:OCR_SIDECAR_PATH = "E:\CodeHome\Library\sidecars\ocr\ocr_sidecar.py"
```

当前 OCR 阶段支持对队列中的扫描版 PDF 和图片启动本地后台 worker 执行 PaddleOCR 推理，并把结果写入统一知识块。每个模型目录必须包含 `inference.json`、`inference.pdiparams`、`inference.yml`，输入文件当前限制为 50 MB 以内，PDF 默认页数上限为 12 页，可通过 `OCR_MAX_PDF_PAGES` 调整。OCR worker 会启用 sidecar 进度流，PDF 按单页写入队列进度，图片按单段写入队列进度；拆页临时文件由 Rust 创建在受控临时目录中，并在正常结束、取消或超时后清理。取消运行中任务会杀掉当前 sidecar 子进程，并且不会把已取消结果标记为成功。重试策略和表格深度理解会在后续解析模块接入。

## MVP 最短链路

当前可以跑通：

```text
新建真实知识库文件夹 -> 扫描 -> 建索引/摘要 -> 在右侧智能助手提问
```

扫描会识别 `.md`、`.txt`、`.docx`、`.xlsx`、`.pdf` 和常见图片文件。新增或变更的普通文档会自动放入 `document` 后台解析队列；图片文件不会进入普通文档解析队列，会自动进入本地 OCR 队列，等待用户启动 OCR worker。点击建索引/摘要会启动文档解析 worker，将可抽取文本转成统一知识块并写入本地 SQLite。助手回答会先检索本地知识块；如果本机配置了 DeepSeek API Key，会尝试使用 `deepseek-v4-flash` 生成更自然的回答，失败时自动回退到本地检索结果。

当前普通 PDF 解析是 MVP 轻量能力，适合可抽取文本的 PDF。扫描版 PDF 和图片可以排队 OCR，再在解析队列中启动后台 OCR worker，并通过队列刷新查看阶段、页/段进度、错误和取消状态。复杂版面、表格深度理解和 OCR 结果版面还原仍是后续能力。

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
│   ├── 下载OCR模型.ps1
│   └── 检查OCR环境.ps1
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

当前扫描阶段会在后台任务中遍历支持格式文件并计算内容指纹，扫描完成后自动为新增、变更和失败的普通文档创建文档解析任务，并为图片文件创建本地 OCR 任务。超大目录扫描后续仍需要接入文件数量/大小限制和更细粒度的实时事件流。

当前扫描、普通文档解析和 OCR 解析都已经接入后台任务队列、进度和取消；普通文档解析支持失败后再次建索引/摘要重试，OCR 支持扫描版 PDF 与常见图片文件，并包含输入大小、PDF 页数限制和页/段进度流。后续需要继续接入更高保真的解析 sidecar、复杂版面还原和表格深度理解。
