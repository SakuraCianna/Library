from __future__ import annotations

from dataclasses import dataclass
import json
from pathlib import Path
import sys
from typing import Any


@dataclass(frozen=True)
class OcrRequest:
    file_path: str
    model_dir: str
    tier: str


def parse_request(raw: str) -> OcrRequest:
    payload = json.loads(raw)
    return OcrRequest(
        file_path=str(payload["filePath"]),
        model_dir=str(payload["modelDir"]),
        tier=str(payload.get("tier", "medium")),
    )


def build_error_response(code: str, message: str) -> dict[str, Any]:
    return {"ok": False, "error": {"code": code, "message": message}}


def build_success_response(text: str, page_count: int) -> dict[str, Any]:
    return {"ok": True, "result": {"text": text, "pageCount": page_count}}


def run_ocr(request: OcrRequest) -> dict[str, Any]:
    file_path = Path(request.file_path)
    model_dir = Path(request.model_dir)
    if not file_path.is_file():
        return build_error_response("INPUT_NOT_FOUND", "输入文件不存在")
    if not model_dir.is_dir():
        return build_error_response("OCR_MODEL_MISSING", "模型目录不存在")

    return build_error_response(
        "OCR_ENGINE_NOT_INSTALLED",
        "OCR 引擎依赖尚未安装，当前只验证 sidecar 协议",
    )


def main() -> int:
    raw = sys.stdin.read()
    try:
        request = parse_request(raw)
        response = run_ocr(request)
    except Exception as exc:
        response = build_error_response("OCR_SIDECAR_ERROR", str(exc))

    sys.stdout.write(json.dumps(response, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
