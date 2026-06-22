# 个人知识库桌面应用

这是个人知识库桌面应用的第一阶段工程骨架。

## 当前已实现

- Tauri v2 桌面应用骨架
- React/Vite/TypeScript 前端
- 中文三栏知识工作台界面
- Rust Tauri 命令边界
- SQLite 本地元数据 schema
- LanceDB 本地向量库连接骨架
- 前端工作台状态读取和浏览器预览降级
- 真实文件夹扫描、文档解析队列和 OCR 队列
- DeepSeek 运行时配置读取和本地检索问答回退
- OCR 模型状态展示和环境自检入口

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

当前仍未实现表格深度问答、回收站执行流、备份导入导出和 OCR 版面还原。
