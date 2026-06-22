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

## 请求格式

```json
{
  "filePath": "E:\\Knowledge\\scan.pdf",
  "modelDir": "E:\\CodeHome\\Library\\models\\ocr\\pp-ocrv6",
  "tier": "medium"
}
```
