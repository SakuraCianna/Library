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
- 安装 Python 3.13，并启用 pip 缓存
- 安装 Protocol Buffers compiler，并写入 `PROTOC`
- 安装 Rust stable 和 `rustfmt`
- 运行 `npm ci`
- 运行 `npm test`
- 运行 `npm run build`
- 在 `sidecars/ocr` 安装 `requirements-dev.txt`
- 运行 `python -m pytest` 验证 OCR sidecar 协议和环境自检脚本
- 在 `sidecars/parser` 安装 `requirements-dev.txt`
- 运行 `python -m pytest` 验证文档解析 sidecar 协议、格式支持和错误边界
- 运行 `cargo fmt -- --check`
- 运行 `cargo test`
- 运行 `cargo check`

CI 只需要 `contents: read` 权限，不读取 DeepSeek API Key，不下载 OCR 模型，不访问个人知识库数据。OCR 和文档解析 sidecar 测试只使用轻量单元测试和临时 fixture，不依赖本机真实模型目录，也不会把文件内容发送到云端服务。

## Release

文件：`.github/workflows/release.yml`

触发条件：

- 推送形如 `v0.1.0` 的 tag
- 手动触发 `workflow_dispatch`

执行内容：

- 使用 `windows-latest` 构建 Windows 桌面端 NSIS 安装包
- 安装 Node.js、Rust stable 和 `protoc`
- 运行 `npm ci`
- 运行 `npm run tauri build -- --no-sign --bundles nsis`，显式生成未签名 NSIS 构建
- 校验 `app/src-tauri/target/release/bundle` 只包含 `nsis` 目标，并将 `bundle/nsis/**` 复制到 `release-assets/windows/nsis`
- 写入 `release-assets/release-manifest.txt`，记录产品名、版本、来源 ref、commit、run id、是否未签名、bundle 目标和保留天数
- 上传 `release-assets/**` 为 GitHub Actions artifact
- 当触发来源是 tag push 时，单独使用最小写权限创建 GitHub Release 草稿并上传构建产物

手动触发 `Release` 时只构建并上传 artifact，不创建 GitHub Release 草稿。

### Release artifact 约定

Actions artifact 名称格式：

```text
library-windows-v<tauri-version>-<safe-ref>-run<run-number>
```

其中 `<tauri-version>` 来自 `app/src-tauri/tauri.conf.json`，`<safe-ref>` 来自当前 tag 或分支名，并会把 `/` 等不适合 artifact 名称的字符替换为 `-`。例如在 `codex/Library` 分支手动 dry run 时，artifact 名称会接近：

```text
library-windows-v0.1.0-codex-Library-run123
```

artifact 内容固定包含：

- `release-manifest.txt`
- `windows/nsis/**`，即 Tauri 生成的 Windows NSIS bundle 文件

artifact 保留 30 天。GitHub Actions artifact 保留策略允许设置 1 到 90 天；本项目使用 30 天，便于回看最近 dry run，同时避免长期占用 artifact 存储。

### 未签名构建限制

当前发布产物未接入 Windows 代码签名，也没有接入自动更新。Release workflow 当前只构建未签名 NSIS 安装包；MSI/WiX、代码签名和自动更新仍属于后续独立模块。未签名构建只适合仓库维护者做本机验收，不应作为可信正式版本对外分发。Windows 或浏览器可能显示未知发布者、SmartScreen 或类似警告；只有确认 artifact 来自本仓库可信 workflow run 后，才应继续手动安装验收。

代码签名证书、签名密钥、自动更新私钥和分发渠道配置必须作为后续独立模块处理，不写入仓库、README、PR 描述或 workflow 日志。

### 手动 installer smoke test

每次准备发版前，至少完成一次 Windows 手动安装验收：

1. 在 GitHub Actions 的 `Release` workflow run 页面下载最新 `library-windows-v<version>-<safe-ref>-run<run-number>` artifact。
2. 解压后先打开 `release-manifest.txt`，确认 `version`、`commit`、`sourceRef`、`unsigned=true` 和 `bundleTargets=nsis` 符合预期。
3. 在 `windows` 目录中选择 Tauri 生成的安装包执行安装；优先使用普通测试账户或一次性测试环境，不要覆盖正在使用的生产知识库目录。
4. 启动“个人知识库”，确认主窗口可打开，设置页可进入，模型/OCR 状态不会显示原始密钥。
5. 使用一个临时空文件夹创建知识库，执行扫描，确认不会报启动或权限边界错误。
6. 关闭应用并卸载测试安装包；如果安装器或系统安全提示与预期不一致，先记录 workflow run、artifact 名称和 manifest，再停止发布。

### Release dry run

`workflow_dispatch` 已配置在默认分支，因此可以从 GitHub CLI 手动运行。验证当前开发分支的 release workflow 时使用：

```powershell
gh workflow run release.yml --repo SakuraCianna/Library --ref codex/Library
gh run list --repo SakuraCianna/Library --workflow Release --branch codex/Library --limit 1
```

随后用返回的 run id 观察结果：

```powershell
gh run watch <run-id> --repo SakuraCianna/Library
gh run view <run-id> --repo SakuraCianna/Library --json conclusion,status,url
```

dry run 只证明 workflow 能在 GitHub Windows runner 上构建并上传 artifact；它不等同于本机安装验收。正式 tag 发布前仍需要下载 artifact 并执行上面的手动 installer smoke test。

本机直接运行 `npm run tauri build` 也需要可用的 `protoc.exe`，并且要么已经在 `PATH` 中，要么通过当前 PowerShell 会话的 `PROTOC` 环境变量显式指定。GitHub `Release` workflow 会在 runner 上安装 `protoc` 并写入 `PROTOC`，所以本机缺少 `protoc` 时，应以 workflow dry run 作为发布构建链路的权威验证。

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
