from __future__ import annotations

from dataclasses import dataclass
import contextlib
import json
import os
from pathlib import Path
import sys
import tempfile
from typing import Any, Callable, Iterable


OCR_VERSION = "PP-OCRv6"
SUPPORTED_EXTENSIONS = {".pdf", ".png", ".jpg", ".jpeg", ".bmp", ".tif", ".tiff", ".webp"}
MAX_INPUT_BYTES = 50 * 1024 * 1024
DEFAULT_MAX_PDF_PAGES = 12
REQUIRED_MODEL_FILES = ("inference.json", "inference.pdiparams", "inference.yml")


@dataclass(frozen=True)
class OcrRequest:
    file_path: str
    model_dir: str
    tier: str
    max_pdf_pages: int
    progress: bool = False
    temp_dir: str | None = None


def parse_request(raw: str) -> OcrRequest:
    payload = json.loads(raw)
    return OcrRequest(
        file_path=str(payload["filePath"]),
        model_dir=str(payload["modelDir"]),
        tier=str(payload.get("tier", "medium")),
        max_pdf_pages=int(payload.get("maxPdfPages") or DEFAULT_MAX_PDF_PAGES),
        progress=bool(payload.get("progress", False)),
        temp_dir=str(payload["tempDir"]) if payload.get("tempDir") else None,
    )


def build_error_response(code: str, message: str) -> dict[str, Any]:
    return {"ok": False, "error": {"code": code, "message": message}}


def build_success_response(text: str, pages: list[dict[str, Any]]) -> dict[str, Any]:
    return {
        "ok": True,
        "result": {
            "text": text,
            "pageCount": len(pages),
            "pages": pages,
        },
    }


def required_model_paths(request: OcrRequest) -> tuple[Path, Path]:
    model_dir = Path(request.model_dir)
    return (
        model_dir / f"{OCR_VERSION}_{request.tier}_det",
        model_dir / f"{OCR_VERSION}_{request.tier}_rec",
    )


def missing_model_assets(request: OcrRequest) -> list[str]:
    missing: list[str] = []
    for model_path in required_model_paths(request):
        if not model_path.is_dir():
            missing.append(model_path.name)
            continue
        for file_name in REQUIRED_MODEL_FILES:
            if not (model_path / file_name).is_file():
                missing.append(f"{model_path.name}/{file_name}")
    return missing


def detect_pdf_page_count(file_path: Path) -> int:
    try:
        from pypdf import PdfReader
    except Exception as exc:  # pragma: no cover - dependency guard
        raise RuntimeError(f"pypdf 未安装或无法导入：{exc}") from exc

    reader = PdfReader(file_path)
    return len(reader.pages)


def build_paddleocr_kwargs(request: OcrRequest) -> dict[str, Any]:
    det_dir, rec_dir = required_model_paths(request)
    return {
        "text_detection_model_name": det_dir.name,
        "text_detection_model_dir": str(det_dir),
        "text_recognition_model_name": rec_dir.name,
        "text_recognition_model_dir": str(rec_dir),
        "use_doc_orientation_classify": False,
        "use_doc_unwarping": False,
        "use_textline_orientation": False,
        # PaddleOCR 3.7 + PaddlePaddle 3.3 can hit a Windows oneDNN PIR
        # runtime bug with PP-OCRv6 medium. Keep CPU inference on plain Paddle.
        "enable_mkldnn": False,
        "device": "cpu",
    }


def validate_request(request: OcrRequest) -> dict[str, Any] | None:
    file_path = Path(request.file_path)
    if not file_path.is_file():
        return build_error_response("INPUT_NOT_FOUND", "输入文件不存在")
    if file_path.stat().st_size > MAX_INPUT_BYTES:
        return build_error_response("OCR_INPUT_TOO_LARGE", "OCR 输入文件超过 50 MB")
    extension = file_path.suffix.lower()
    if extension not in SUPPORTED_EXTENSIONS:
        return build_error_response("OCR_UNSUPPORTED_FILE", "当前 OCR 仅支持 PDF 或图片文件")
    if extension == ".pdf":
        try:
            page_count = detect_pdf_page_count(file_path)
        except RuntimeError as exc:
            return build_error_response("OCR_RUNTIME_NOT_INSTALLED", str(exc))
        except Exception as exc:
            return build_error_response("OCR_PDF_PAGE_COUNT_FAILED", f"无法读取 PDF 页数：{exc}")
        if page_count > request.max_pdf_pages:
            return build_error_response(
                "OCR_TOO_MANY_PAGES",
                f"OCR PDF 页数 {page_count} 超过当前上限 {request.max_pdf_pages}",
            )

    missing = missing_model_assets(request)
    if missing:
        return build_error_response(
            "OCR_MODEL_MISSING",
            "模型目录不完整，缺少 " + "、".join(missing),
        )

    return None


def build_real_ocr_engine(request: OcrRequest) -> Callable[[str], Iterable[Any]]:
    os.environ["DISABLE_MODEL_SOURCE_CHECK"] = "True"
    os.environ["PADDLE_PDX_DISABLE_MODEL_SOURCE_CHECK"] = "True"
    try:
        with contextlib.redirect_stdout(sys.stderr):
            from paddleocr import PaddleOCR
    except Exception as exc:  # pragma: no cover - covered through run_ocr injection
        raise RuntimeError(f"OCR runtime 未安装或无法导入：{exc}") from exc

    with contextlib.redirect_stdout(sys.stderr):
        ocr = PaddleOCR(**build_paddleocr_kwargs(request))

    def predict(path: str) -> Iterable[Any]:
        with contextlib.redirect_stdout(sys.stderr):
            return ocr.predict(path)

    return predict


def emit_stream_event(event: dict[str, Any]) -> None:
    print(json.dumps(event, ensure_ascii=False), flush=True)


def emit_progress(
    callback: Callable[[dict[str, Any]], None] | None,
    *,
    phase: str,
    current: int,
    total: int,
) -> None:
    if callback is not None:
        callback({"phase": phase, "current": current, "total": total})


def extract_ocr_pages(
    raw_results: Iterable[Any],
    *,
    forced_page_index: int | None = None,
) -> list[dict[str, Any]]:
    pages = []
    for fallback_index, item in enumerate(raw_results):
        payload = item.json if hasattr(item, "json") else item
        if isinstance(payload, str):
            payload = json.loads(payload)
        if not isinstance(payload, dict):
            continue

        result = payload.get("res", payload)
        texts = [str(text).strip() for text in result.get("rec_texts", [])]
        texts = [text for text in texts if text]
        if not texts:
            continue

        scores = [float(score) for score in result.get("rec_scores", [])]
        confidence = None
        if scores:
            confidence = round(sum(scores) / len(scores), 3)
        if forced_page_index is None:
            page_index = result.get("page_index")
            if page_index is None:
                page_index = fallback_index
        else:
            page_index = forced_page_index + fallback_index

        pages.append(
            {
                "pageIndex": int(page_index),
                "text": "\n".join(texts),
                "confidence": confidence,
            }
        )

    return pages


def write_single_page_pdf(source_pdf: Path, target_pdf: Path, page_index: int) -> None:
    from pypdf import PdfReader, PdfWriter

    reader = PdfReader(source_pdf)
    writer = PdfWriter()
    writer.add_page(reader.pages[page_index])
    with target_pdf.open("wb") as file:
        writer.write(file)


@contextlib.contextmanager
def ocr_page_work_dir(request: OcrRequest):
    if request.temp_dir:
        path = Path(request.temp_dir)
        path.mkdir(parents=True, exist_ok=True)
        yield path
        return

    with tempfile.TemporaryDirectory(prefix="library-ocr-") as temp_dir:
        yield Path(temp_dir)


def run_pdf_ocr_by_page(
    *,
    work_dir: Path,
    file_path: Path,
    engine: Callable[[str], Iterable[Any]],
    progress_callback: Callable[[dict[str, Any]], None] | None,
) -> list[dict[str, Any]]:
    page_count = detect_pdf_page_count(file_path)
    pages: list[dict[str, Any]] = []

    for page_index in range(page_count):
        page_number = page_index + 1
        page_pdf = work_dir / f"page-{page_number}.pdf"
        write_single_page_pdf(file_path, page_pdf, page_index)

        emit_progress(
            progress_callback,
            phase=f"正在识别第 {page_number}/{page_count} 页",
            current=page_index,
            total=page_count,
        )
        pages.extend(
            extract_ocr_pages(engine(str(page_pdf)), forced_page_index=page_index)
        )
        emit_progress(
            progress_callback,
            phase=f"已识别第 {page_number}/{page_count} 页",
            current=page_number,
            total=page_count,
        )

    return pages


def run_image_ocr(
    *,
    file_path: Path,
    engine: Callable[[str], Iterable[Any]],
    progress_callback: Callable[[dict[str, Any]], None] | None,
) -> list[dict[str, Any]]:
    emit_progress(progress_callback, phase="正在识别图片", current=0, total=1)
    pages = extract_ocr_pages(engine(str(file_path)), forced_page_index=0)
    emit_progress(progress_callback, phase="已识别图片", current=1, total=1)
    return pages


def run_ocr(
    request: OcrRequest,
    ocr_factory: Callable[[OcrRequest], Callable[[str], Iterable[Any]]] | None = None,
    progress_callback: Callable[[dict[str, Any]], None] | None = None,
) -> dict[str, Any]:
    validation_error = validate_request(request)
    if validation_error is not None:
        return validation_error

    try:
        engine = (ocr_factory or build_real_ocr_engine)(request)
        file_path = Path(request.file_path)
        if file_path.suffix.lower() == ".pdf":
            with ocr_page_work_dir(request) as work_dir:
                pages = run_pdf_ocr_by_page(
                    work_dir=work_dir,
                    file_path=file_path,
                    engine=engine,
                    progress_callback=progress_callback,
                )
        else:
            pages = run_image_ocr(
                file_path=file_path,
                engine=engine,
                progress_callback=progress_callback,
            )
    except RuntimeError as exc:
        return build_error_response("OCR_RUNTIME_NOT_INSTALLED", str(exc))
    except Exception as exc:
        return build_error_response("OCR_RUNTIME_ERROR", str(exc))

    if not pages:
        return build_error_response("OCR_EMPTY_RESULT", "没有从文件中识别到文字")

    text = "\n\n".join(page["text"] for page in pages)
    return build_success_response(text, pages)


def main() -> int:
    raw = sys.stdin.read()
    try:
        request = parse_request(raw)
        if request.progress:
            response = run_ocr(
                request,
                progress_callback=lambda progress: emit_stream_event(
                    {"type": "progress", **progress}
                ),
            )
            emit_stream_event({"type": "result", "response": response})
            return 0

        response = run_ocr(request)
    except Exception as exc:
        response = build_error_response("OCR_SIDECAR_ERROR", str(exc))

    sys.stdout.write(json.dumps(response, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
