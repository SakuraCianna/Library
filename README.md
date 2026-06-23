# 个人知识库桌面应用

一个本地优先的 Windows 桌面端个人知识库应用。目标是让用户把 Word、PDF、XLSX、Markdown 等资料放入真实文件夹后，由应用建立本地元数据、全文检索、向量索引和智能助手工作台，逐步形成长期个人知识资产。

当前仓库处于第一阶段工程骨架后的稳定推进阶段：已经完成桌面壳、中文三栏界面、Rust 命令边界、本地 SQLite 元数据骨架、本地 LanceDB 向量库骨架、真实文件夹登记、文件元数据扫描入库、本地文档解析 sidecar、普通文档和 XLSX 工作表结构洞察、本地备份导出与受保护恢复，以及来源更可靠的智能助手最短链路。

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
- 对 `.xlsx` 提取工作表级结构洞察，包括行列规模、表头、样例行和可问答字段
- 对文本型 `.pdf` 做 MVP 轻量文本抽取
- 本地文档解析 sidecar JSON 协议，支持 `.md`、`.txt`、文本型 `.pdf`、`.docx` 和 `.xlsx`
- 将解析结果写入统一的 `knowledge_blocks` 结构化数据层
- XLSX 工作表结构洞察会以 `table` 知识块写入统一数据层，并可被本地全文检索命中
- 长文档会按内容切分为多个可检索知识块，并生成 `#block-N` 来源定位
- 为知识块建立 SQLite FTS5 全文索引和摘要预览
- 右侧表格理解预览会显示最新解析到的真实 XLSX 工作表洞察
- 侧边栏智能助手可基于本地索引检索并回答
- 侧边栏智能助手回答会保留命中文件、知识块摘要和相对定位作为来源证据
- 侧边栏智能助手会标注来源类型，区分原始文件、Markdown 笔记、XLSX 表格洞察和本地 OCR
- 表格洞察和本地 OCR 来源在相关问题中会优先排到更靠前的位置
- 本地兜底回答在没有命中证据时会明确说明没有足够本地证据，不编造答案
- 回答来源区可隐藏，也可按全部、原始文件、Markdown 笔记、表格洞察和本地 OCR 筛选
- 聊天来源卡片可切换右侧知识块预览，查看对应来源详情和定位
- 来源详情可打开对应本地文件，路径解析由 Rust 校验在当前知识库目录内
- 来源详情可在同一文件的多个知识块片段之间切换，查看命中片段前后上下文
- DeepSeek Chat Completions 调用边界，失败或未配置时使用本地检索兜底回答
- Rust Tauri 命令边界
- 文件夹默认权限与当前会话权限分离
- 左下角文件夹默认权限下拉设置
- 设置模态框集中展示常规、模型/OCR 和权限状态
- 右侧当前会话权限下拉设置
- SQLite 本地元数据 schema 和 repository 骨架
- SQLite FTS5 全文检索基础表和触发器
- LanceDB 本地向量库连接骨架
- LanceDB 本地路径边界校验，拒绝远程或云端 URI
- 浏览器环境空状态 fallback，Tauri 环境读取真实本地工作台状态
- Iconify 图标组件和本地 lucide 单图标依赖
- GitHub Actions CI 工作流
- Windows 桌面端 Release NSIS 构建工作流
- Release artifact 使用可预测命名、30 天保留和 `release-manifest.txt` 元数据
- CI/CD 文档记录未签名构建限制、Release dry run 和手动 installer smoke test 步骤
- DeepSeek 本地安全配置状态读取
- PP-OCRv6 本地模型下载脚本和状态检查
- 桌面应用内 OCR 环境自检入口
- 本地 OCR sidecar JSON 协议和 PaddleOCR 真实推理入口
- 图片文件可进入本地 OCR 队列并写入统一知识块
- SQLite 解析队列入队、后台执行、取消、进度和列表查询骨架
- 扫描文件夹接入后台任务队列、粗粒度进度和取消
- 扫描后自动为待解析文档创建后台解析任务
- 扫描后自动为图片文件创建本地 OCR 任务
- 普通文档解析后台 worker，可优先调用本地 parser sidecar 逐个处理 `.pdf`、`.docx`、`.xlsx`、`.md`、`.txt`
- 文档解析 sidecar 不下载模型、不调用云服务，Rust 仍负责路径校验和数据库写入
- 普通文档解析失败后可通过再次建索引/摘要手动重试
- 前端解析队列状态展示和运行中任务轮询刷新
- 前端可启动后台 OCR worker、刷新进度并取消排队或运行中的 OCR 任务
- 后台扫描、文档解析和 OCR worker 会通过 Tauri 事件通知前端刷新队列状态，并保留轮询兜底
- OCR sidecar 可输出页/段进度流，PDF 按页更新队列进度，图片按单段更新队列进度
- 可将当前知识库的空间配置、文件元数据、Markdown 笔记元数据、知识块、解析任务和回收站记录导出为应用数据目录 `backups` 下的 JSON 备份
- 备份导出拒绝路径穿越文件名，并且不导出密钥、`.env`、OCR 模型目录或临时文件
- 可选择 JSON 备份文件进行恢复预检，先校验格式版本、内部相对路径和引用结构，再由用户显式确认覆盖恢复
- 备份恢复会在本地 SQLite 事务中替换同 ID 知识库，并在成功后刷新工作台状态
- 前端和 Rust 单元测试

### 权限说明

默认权限是某个知识库文件夹长期保存的 Agent 操作边界，保存在本地 SQLite 中。它决定这个文件夹默认允许助手做到哪一步。

当前会话权限只影响右侧智能助手的本次聊天，不会写回文件夹默认配置。会话权限不能超过文件夹默认权限允许的上限；高风险操作后续仍会要求二次确认。

### 暂未实现

- 高保真 PDF 版面解析
- 复杂表格公式、合并单元格和跨表问答
- DeepSeek 流式输出
- 回收站执行流
- Windows 代码签名和自动更新

## 当前进度验收

当前主线处于第一阶段本地优先 MVP 后的稳定推进阶段。Module 7 发布准备已将 Windows Release artifact 命名、保留策略、manifest 和未签名验收说明落到 workflow 与文档中；Release runner dry run 和 installer smoke test 仍按 `docs/ci-cd.md` 执行。后续模块继续按 `docs/superpowers/plans/2026-06-23-library-sustained-stability-roadmap.md` 推进。

本轮验收将 README 功能状态与当前源码、测试和 GitHub Actions 配置重新核对：文件扫描、普通文档解析、parser sidecar、XLSX 表格洞察、本地全文检索、来源证据、来源排序、来源显示控制、DeepSeek 兜底问答、本地 OCR sidecar、解析队列、后台 worker、进度、取消、事件刷新、本地备份导出、受保护恢复、CI 门禁和 Release artifact 约定都有对应命令边界、前端入口、单元测试或 workflow 配置覆盖。未在当前源码中实现的高保真解析、复杂表格推理、代码签名和自动更新仍保持在“暂未实现”。

## 技术栈

- 桌面端：Tauri v2
- 前端：React、Vite、TypeScript
- 样式：CSS Modules、CSS 变量、Iconify 图标
- Rust 核心：Tauri commands、状态管理、权限边界
- 元数据数据库：SQLite、FTS5
- 向量数据库：LanceDB 本地 embedded 模式
- 测试：Vitest、React Testing Library、pytest、Cargo test

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

DeepSeek API Key 只从本机环境变量或本地未提交的 `.env` 读取，仓库只保留 `.env.example` 占位说明，不提交真实密钥。设置面板只显示密钥是否已配置和脱敏片段，不显示完整密钥。

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
$env:OCR_MAX_IMAGE_PIXELS = "25000000"
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

自检脚本可通过 `-MaxPdfPages` 和 `-MaxImagePixels` 覆盖 smoke 检查限制，桌面应用自检会使用运行时相同的默认限制。

桌面应用设置模态框的“模型与 OCR”页也提供 OCR 自检入口，会检查模型文件、sidecar、`pypdf`、`paddleocr` 和 `paddlepaddle`。

桌面构建会把 `sidecars/ocr/ocr_sidecar.py`、`check_ocr_environment.py` 和 `requirements.txt` 作为 Tauri resource 打包。开发态优先使用仓库根目录下的 `.venv\Scripts\python.exe`，打包后可通过本机环境变量显式指定：

```powershell
$env:OCR_PYTHON_PATH = "E:\CodeHome\Library\.venv\Scripts\python.exe"
$env:OCR_SIDECAR_PATH = "E:\CodeHome\Library\sidecars\ocr\ocr_sidecar.py"
```

当前 OCR 阶段支持对队列中的扫描版 PDF 和图片启动本地后台 worker 执行 PaddleOCR 推理，并把结果写入统一知识块。每个模型目录必须包含 `inference.json`、`inference.pdiparams`、`inference.yml`，输入文件当前限制为 50 MB 以内，PDF 默认页数上限为 12 页，可通过 `OCR_MAX_PDF_PAGES` 调整；图片默认像素上限为 25000000，可通过 `OCR_MAX_IMAGE_PIXELS` 调整。OCR worker 会启用 sidecar 进度流，PDF 按单页写入队列进度，图片按单段写入队列进度；拆页临时文件由 Rust 创建在受控临时目录中，并在正常结束、取消或超时后清理。取消运行中任务会杀掉当前 sidecar 子进程，并且不会把已取消结果标记为成功。失败的文档解析和 OCR 任务可重新排队，队列会复用仍在等待或运行的 active job，避免重复 active 任务。

文档解析 sidecar 使用同一个项目本地 Python 环境，不需要模型目录，也不会联网下载模型或发送文件内容。开发态可以安装 parser 测试依赖并单独运行协议测试：

```powershell
.\.venv\Scripts\python.exe -m pip install -r .\sidecars\parser\requirements-dev.txt
Set-Location .\sidecars\parser
..\..\.venv\Scripts\python.exe -m pytest
Set-Location ..\..
```

桌面构建会把 `sidecars/parser/parser_sidecar.py` 和 `requirements.txt` 作为 Tauri resource 打包。开发态默认优先发现仓库根目录的 `.venv\Scripts\python.exe` 和 `sidecars\parser\parser_sidecar.py`；打包后也可通过本机环境变量显式指定：

```powershell
$env:PARSER_PYTHON_PATH = "E:\CodeHome\Library\.venv\Scripts\python.exe"
$env:PARSER_SIDECAR_PATH = "E:\CodeHome\Library\sidecars\parser\parser_sidecar.py"
```

## MVP 最短链路

当前可以跑通：

```text
新建真实知识库文件夹 -> 扫描 -> 建索引/摘要 -> 在右侧智能助手提问 -> 导出本地备份 -> 预检并确认恢复备份
```

扫描会识别 `.md`、`.txt`、`.docx`、`.xlsx`、`.pdf` 和常见图片文件。新增或变更的普通文档会自动放入 `document` 后台解析队列；图片文件不会进入普通文档解析队列，会自动进入本地 OCR 队列，等待用户启动 OCR worker。点击建索引/摘要会启动文档解析 worker，优先通过本地 parser sidecar 将可抽取文本转成统一知识块并写入本地 SQLite；如果 sidecar 未配置或不可用，会回退到 Rust 轻量解析。长文档会切分成多个知识块，让检索命中更接近回答依据。`.xlsx` 还会额外生成工作表级 table 知识块，记录行列规模、表头和样例行；右侧表格理解预览会显示最新表格洞察，聊天检索也能命中这些字段。助手回答会先检索本地知识块，并在回答与来源卡片中标注命中内容是原始文件、Markdown 笔记、表格洞察还是本地 OCR；表格问题会优先展示表格洞察，扫描版/OCR 问题会优先展示 OCR 片段。如果本地索引没有命中证据，本地兜底回答会明确说明没有足够本地证据且不会编造；如果本机配置了 DeepSeek API Key，会尝试使用 `deepseek-v4-flash` 生成更自然的回答，失败时自动回退到本地检索结果。无论使用 DeepSeek 还是本地兜底，右侧聊天都会保留本次命中的来源文件、知识块标题、摘要证据和相对来源定位，不展示知识库根目录绝对路径；来源区可以隐藏，也可以按原始文件、Markdown 笔记、表格洞察和本地 OCR 筛选。点击聊天来源卡片后，右侧知识块预览会切换到对应来源详情，可在同一文件的多个片段之间切换上下文，可打开本地源文件，也可再返回最新知识块。打开文件前会由 Rust 使用知识库根目录和相对定位解析真实路径，并拒绝绝对路径或越界路径；`#block-N`、`#sheet-N` 和 OCR 片段定位只用于证据显示，不会被当成真实文件名打开。

当前普通 PDF 解析适合可抽取文本的 PDF；parser sidecar 会先尝试 `pypdf` 文本抽取，再使用轻量文本 fallback。扫描版 PDF 和图片可以排队 OCR，再在解析队列中启动后台 OCR worker，并通过队列刷新查看阶段、页/段进度、错误和取消状态；队列错误会在界面上隐藏本机绝对路径并限制长度。工具栏的“导出备份”会把当前知识库的本地数据库元数据写入应用数据目录下的 `backups` 子目录；“恢复备份”会先选择 JSON 文件并做只读预检，确认格式版本、相对路径和对象引用都可接受后，仍需要用户点击“确认恢复”才会覆盖同 ID 知识库的本地元数据。复杂版面、复杂表格推理和 OCR 结果版面还原仍是后续能力。

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

文档解析 sidecar 测试：

```powershell
Set-Location .\sidecars\parser
..\..\.venv\Scripts\python.exe -m pytest
```

## CI/CD

仓库提供两条 GitHub Actions 工作流：

- `CI`：PR、`main`、`codex/**` 分支推送和手动触发时运行，检查前端测试、前端构建、OCR sidecar 测试、文档解析 sidecar 测试、Rust 格式、Rust 测试和 Rust 编译
- `Release`：推送 `v*.*.*` tag 或手动触发时运行，使用 `npm run tauri build -- --no-sign --bundles nsis` 构建未签名 Windows NSIS 安装产物，上传带 manifest 的 `library-windows-v<version>-<safe-ref>-run<run-number>` artifact，保留 30 天；tag 触发时会创建 GitHub Release 草稿

详细说明、未签名构建限制、Release dry run 和手动安装验收步骤见 [CI/CD 工作流](docs/ci-cd.md)。

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
│   ├── ocr
│   └── parser
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

当前 Module 7 本地验收已通过以下检查，最近一次本地验收时间为 2026-06-23：

- `Set-Location .\app; npm test`：4 个测试文件、33 个测试通过
- `Set-Location .\app; npm run build`：TypeScript 和 Vite 构建通过
- `Set-Location .\sidecars\ocr; ..\..\.venv\Scripts\python.exe -m pytest`：25 个 OCR sidecar 和环境自检测试通过
- `Set-Location .\sidecars\parser; ..\..\.venv\Scripts\python.exe -m pytest`：10 个文档解析 sidecar 测试通过
- `Set-Location .\app\src-tauri; cargo fmt -- --check`：Rust 格式检查通过
- `Set-Location .\app\src-tauri; cargo test`：110 个 Rust 测试通过
- `Set-Location .\app\src-tauri; cargo check`：Rust 编译检查通过
- `Set-Location E:\CodeHome\Library; git diff --check`：空白字符检查通过
- 使用 PyYAML 解析 `.github/workflows/ci.yml` 和 `.github/workflows/release.yml`：通过

本机直接运行 `Set-Location .\app; npm run tauri build` 时，前端构建通过，但 Rust release bundle 构建在 `lance-encoding` 依赖阶段失败，原因是当前本机未找到 `protoc.exe`，且当前 PowerShell 会话没有设置 `PROTOC`。GitHub `CI` 和 `Release` workflow 会在 Windows runner 中安装 `protoc` 并设置 `PROTOC`；本机运行 Tauri release build 仍需要开发者自行准备 `protoc.exe`。

当前扫描阶段会在后台任务中遍历支持格式文件并计算内容指纹，扫描完成后自动为新增、变更和失败的普通文档创建文档解析任务，并为图片文件创建本地 OCR 任务。扫描默认限制为最多 10000 个支持格式文件、支持格式文件总大小最多 10 GB；超过上限会以中文错误停止扫描，不会在队列错误里展示知识库根目录绝对路径。更细粒度的实时事件流仍是后续能力。

当前扫描、普通文档解析和 OCR 解析都已经接入后台任务队列、进度和取消；普通文档解析支持失败后再次建索引/摘要重试，OCR 支持扫描版 PDF 与常见图片文件，并包含输入大小、PDF 页数、图片像素限制和页/段进度流；备份恢复已经具备预检、显式确认和事务替换基础；助手回答已经具备来源排序、无证据边界和来源显示控制。后续需要继续推进发布 dry run、高保真版面解析、复杂表格推理和 OCR 结果版面还原。
