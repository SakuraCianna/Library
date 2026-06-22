# OCR Sidecar

本目录保存本地 OCR sidecar。Rust 主进程通过 stdin 传入 JSON，通过 stdout 读取 JSON 响应。

## 本地测试

```powershell
Set-Location .\sidecars\ocr
python -m pip install -r requirements-dev.txt
python -m pytest
```

真实 OCR 依赖单独安装：

```powershell
Set-Location .\sidecars\ocr
python -m pip install -r requirements.txt
```

安装依赖和下载模型后，可以在仓库根目录运行本地自检：

```powershell
.\scripts\检查OCR环境.ps1 -Tier medium
```

当前运行时固定使用 PaddleOCR 3.7.x。该版本可以加载本仓库下载脚本准备的 `PP-OCRv6_medium_det` 和 `PP-OCRv6_medium_rec` 本地模型目录，并支持 PDF 与常见图片文件。

Windows CPU 环境下 sidecar 会显式关闭 MKL-DNN，并且只从传入的本地模型目录加载模型；如果模型目录或 `inference.json`、`inference.pdiparams`、`inference.yml` 缺失，会返回 JSON 错误而不是自动下载。sidecar 同时设置 `PADDLE_PDX_DISABLE_MODEL_SOURCE_CHECK=True`，避免 PaddleX 走模型源检查路径。

默认输入文件上限为 50 MB。PDF 会额外检查页数限制；超过上限时会返回 `OCR_INPUT_TOO_LARGE` 或 `OCR_TOO_MANY_PAGES`。

## 请求格式

```json
{
  "filePath": "E:\\Knowledge\\scan.pdf",
  "modelDir": "E:\\CodeHome\\Library\\models\\ocr\\pp-ocrv6",
  "tier": "medium",
  "progress": true,
  "tempDir": "C:\\Users\\Sakura_Cianna\\AppData\\Local\\Temp\\library-ocr-runs\\run-..."
}
```

默认不传 `progress` 时，stdout 返回单个 JSON 响应，保持脚本级调用兼容。传入 `"progress": true` 时，stdout 使用 NDJSON：先输出 `{"type":"progress",...}` 事件，最后输出 `{"type":"result","response":...}`。PDF 会拆成单页临时文件逐页识别，图片作为一个分段识别；PaddleOCR 日志仍然只写 stderr。`tempDir` 是可选的调用方拥有目录，桌面端由 Rust 在受控临时根目录下创建并在正常结束、取消或超时后清理。

## 本地 smoke

仓库不提交二进制 OCR fixture。需要验证真实推理时，可以在项目根目录创建 `.venv`，安装 `sidecars/ocr/requirements.txt`，先运行模型下载脚本，再用本机生成的扫描版 PDF 或图片通过 stdin 调用 `ocr_sidecar.py`。

也可以把扫描版 PDF 或图片交给自检脚本触发一次真实 sidecar smoke：

```powershell
.\scripts\检查OCR环境.ps1 -Tier medium -SmokeFile "E:\Knowledge\scan.png"
```

sidecar 的 stdout 必须保持为 JSON 数据：默认是单个 JSON 响应，启用 `progress` 后是逐行 JSON 事件；PaddleOCR 初始化日志会被重定向到 stderr，避免污染 Rust 主进程解析。
