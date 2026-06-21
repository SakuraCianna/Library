# CI/CD 工作流

本项目使用 GitHub Actions 维护两条工作流：`CI` 负责每次 PR 和分支推送的质量门禁，`Release` 负责构建 Windows 桌面端安装产物。

## CI

文件：`.github/workflows/ci.yml`

触发条件：

- 向 `main` 发起 Pull Request
- 推送到 `main`
- 推送到 `codex/**`
- 手动触发 `workflow_dispatch`

执行内容：

- 使用 `windows-latest`，贴近当前 Windows 桌面应用目标环境
- 安装 Node.js 22，并启用 npm 缓存
- 安装 Protocol Buffers compiler，并写入 `PROTOC`
- 安装 Rust stable 和 `rustfmt`
- 运行 `npm ci`
- 运行 `npm test`
- 运行 `npm run build`
- 运行 `cargo fmt -- --check`
- 运行 `cargo test`
- 运行 `cargo check`

CI 只需要 `contents: read` 权限，不读取 DeepSeek API Key，不下载 OCR 模型，不访问个人知识库数据。

## Release

文件：`.github/workflows/release.yml`

触发条件：

- 推送形如 `v0.1.0` 的 tag
- 手动触发 `workflow_dispatch`

执行内容：

- 使用 `windows-latest` 构建 Windows 桌面端包
- 安装 Node.js、Rust stable 和 `protoc`
- 运行 `npm ci`
- 运行 `npm run tauri build`
- 上传 `app/src-tauri/target/release/bundle/**` 为 GitHub Actions artifact
- 当触发来源是 tag push 时，单独使用最小写权限创建 GitHub Release 草稿并上传构建产物

手动触发 `Release` 时只构建并上传 artifact，不创建 GitHub Release 草稿。

当前发布产物未接入代码签名。正式对外分发前，需要补充 Windows 代码签名证书和安装包验收流程。

## 发版步骤

确认 `main` 处于可发布状态后，在本地执行：

```powershell
git switch main
git pull --ff-only origin main
git tag v0.1.0
git push origin v0.1.0
```

等待 `Release` 工作流完成后，在 GitHub Releases 中检查草稿 Release 的安装产物。确认无误后，再手动发布草稿。

## 维护约定

- 新增启动方式、构建命令、环境变量或发布要求时，同步更新 `README.md` 和本文档
- 新增必须通过的检查时，同步更新 `.github/workflows/ci.yml`
- 新增安装包签名或自动更新时，同步更新 `.github/workflows/release.yml`
- 不把 API Key、token、证书、cookie 或本地模型文件写入工作流和文档
