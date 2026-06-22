# OCR Fixtures

本目录暂不提交二进制 OCR fixture。

真实 OCR smoke 使用本机临时生成的扫描版 PDF，避免把模型输出、字体渲染差异或授权不清晰的样例文件写入仓库。验证时应在项目根目录使用 `.venv` 安装 `sidecars/ocr/requirements.txt`，下载 `PP-OCRv6_medium_det` 和 `PP-OCRv6_medium_rec` 后调用 `sidecars/ocr/ocr_sidecar.py`。

后续如果需要 CI 级 OCR smoke，再添加一个体积很小、来源明确、渲染稳定的生成脚本，而不是提交来源不明的 PDF。
