# 个人知识库桌面应用

一个本地优先的 Windows 桌面端个人知识库应用。目标是让用户把 Word、PDF、XLSX、Markdown 等资料放入真实文件夹后，由应用建立本地元数据、全文检索、向量索引和智能助手工作台，逐步形成长期个人知识资产。

当前仓库处于第一阶段工程骨架：已经完成桌面壳、中文三栏界面、Rust 命令边界、本地 SQLite 元数据骨架、本地 LanceDB 向量库骨架，以及前端状态读取降级链路。

## 功能状态

### 已实现

- Windows 桌面应用骨架，基于 Tauri v2
- React + Vite + TypeScript 前端工程
- 全中文三栏知识工作台界面
- 文件夹式知识空间 mock 数据和前端类型契约
- Rust Tauri 命令边界
- 文件夹默认权限与当前会话权限分离
- SQLite 本地元数据 schema 和 repository 骨架
- SQLite FTS5 全文检索基础表和触发器
- LanceDB 本地向量库连接骨架
- LanceDB 本地路径边界校验，拒绝远程或云端 URI
- 浏览器环境 mock fallback，Tauri 环境读取工作台状态
- 前端和 Rust 单元测试

### 暂未实现

- 真实文件夹扫描
- Word、PDF、XLSX、Markdown 解析
- OCR 和本地模型下载
- 表格深度理解和表格问答
- DeepSeek `deepseek-v4-flash` 调用
- Python 文档解析 Sidecar
- 回收站执行流
- 备份、导出、导入
- 安装包发布流程

## 技术栈

- 桌面端：Tauri v2
- 前端：React、Vite、TypeScript
- 样式：CSS Modules、CSS 变量
- Rust 核心：Tauri commands、状态管理、权限边界
- 元数据数据库：SQLite、FTS5
- 向量数据库：LanceDB 本地 embedded 模式
- 测试：Vitest、React Testing Library、Cargo test

## 快速启动

首次运行前需要安装依赖：

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
cargo fmt --check
cargo test
```

## 目录结构

```text
.
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
│   │       ├── storage
│   │       └── vector
│   └── package.json
├── docs
│   └── superpowers
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

- `npm test`
- `npm run build`
- `cargo fmt --check`
- `cargo test`
- `git diff --check`
