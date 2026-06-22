import json
import os
from pathlib import Path
import sys
import types

from ocr_sidecar import (
    build_error_response,
    build_real_ocr_engine,
    build_paddleocr_kwargs,
    extract_ocr_pages,
    missing_model_assets,
    parse_request,
    required_model_paths,
    run_ocr,
)


def test_parse_request_accepts_local_file_and_model_dir():
    request = parse_request(
        json.dumps(
            {
                "filePath": "E:\\Knowledge\\scan.pdf",
                "modelDir": "E:\\CodeHome\\Library\\models\\ocr\\pp-ocrv6",
                "tier": "medium",
            }
        )
    )

    assert request.file_path.endswith("scan.pdf")
    assert request.model_dir.endswith("pp-ocrv6")
    assert request.tier == "medium"
    assert request.max_pdf_pages == 12


def test_error_response_is_json_serializable():
    response = build_error_response("OCR_MODEL_MISSING", "模型目录不存在")

    assert response["ok"] is False
    assert response["error"]["code"] == "OCR_MODEL_MISSING"
    assert "模型目录不存在" in response["error"]["message"]


def test_required_model_paths_use_ppocrv6_medium_dirs(tmp_path: Path):
    request = parse_request(
        json.dumps(
            {
                "filePath": str(tmp_path / "scan.pdf"),
                "modelDir": str(tmp_path / "models"),
                "tier": "medium",
            }
        )
    )

    det_dir, rec_dir = required_model_paths(request)

    assert det_dir.name == "PP-OCRv6_medium_det"
    assert rec_dir.name == "PP-OCRv6_medium_rec"


def test_missing_model_assets_require_runtime_files(tmp_path: Path):
    model_dir = tmp_path / "models"
    (model_dir / "PP-OCRv6_medium_det").mkdir(parents=True)
    (model_dir / "PP-OCRv6_medium_rec").mkdir(parents=True)
    request = parse_request(
        json.dumps(
            {
                "filePath": str(tmp_path / "scan.pdf"),
                "modelDir": str(model_dir),
                "tier": "medium",
            }
        )
    )

    missing = missing_model_assets(request)

    assert "PP-OCRv6_medium_det/inference.json" in missing
    assert "PP-OCRv6_medium_rec/inference.pdiparams" in missing


def test_build_paddleocr_kwargs_forces_local_models_and_cpu(tmp_path: Path):
    request = parse_request(
        json.dumps(
            {
                "filePath": str(tmp_path / "scan.pdf"),
                "modelDir": str(tmp_path / "models"),
                "tier": "medium",
            }
        )
    )

    kwargs = build_paddleocr_kwargs(request)

    assert kwargs["text_detection_model_name"] == "PP-OCRv6_medium_det"
    assert kwargs["text_recognition_model_name"] == "PP-OCRv6_medium_rec"
    assert kwargs["use_doc_orientation_classify"] is False
    assert kwargs["use_doc_unwarping"] is False
    assert kwargs["use_textline_orientation"] is False
    assert kwargs["enable_mkldnn"] is False
    assert kwargs["device"] == "cpu"


def test_extract_ocr_pages_normalizes_paddle_result_shape():
    raw_results = [
        {
            "res": {
                "page_index": 0,
                "rec_texts": ["HELLO", "OCR"],
                "rec_scores": [0.99, 0.88],
            }
        },
        {
            "res": {
                "page_index": 1,
                "rec_texts": ["PAGE TWO"],
                "rec_scores": [0.77],
            }
        },
    ]

    pages = extract_ocr_pages(raw_results)

    assert pages == [
        {"pageIndex": 0, "text": "HELLO\nOCR", "confidence": 0.935},
        {"pageIndex": 1, "text": "PAGE TWO", "confidence": 0.77},
    ]


def test_run_ocr_uses_injected_engine_without_importing_heavy_runtime(tmp_path: Path):
    pdf_path = tmp_path / "scan.pdf"
    model_dir = tmp_path / "models"
    create_model_assets(model_dir)
    write_test_pdf(pdf_path)
    request = parse_request(
        json.dumps(
            {
                "filePath": str(pdf_path),
                "modelDir": str(model_dir),
                "tier": "medium",
            }
        )
    )

    response = run_ocr(
        request,
        ocr_factory=lambda _request: lambda _path: [
            {"res": {"page_index": 0, "rec_texts": ["LOCAL OCR TEXT"], "rec_scores": [0.9]}}
        ],
    )

    assert response["ok"] is True
    assert response["result"]["text"] == "LOCAL OCR TEXT"
    assert response["result"]["pageCount"] == 1


def test_run_ocr_accepts_image_without_pdf_page_count(tmp_path: Path):
    image_path = tmp_path / "scan.png"
    model_dir = tmp_path / "models"
    create_model_assets(model_dir)
    image_path.write_bytes(b"image")
    request = parse_request(
        json.dumps(
            {
                "filePath": str(image_path),
                "modelDir": str(model_dir),
                "tier": "medium",
                "maxPdfPages": 1,
            }
        )
    )

    response = run_ocr(
        request,
        ocr_factory=lambda _request: lambda _path: [
            {
                "res": {
                    "page_index": 0,
                    "rec_texts": ["IMAGE OCR TEXT"],
                    "rec_scores": [0.91],
                }
            }
        ],
    )

    assert response["ok"] is True
    assert response["result"]["text"] == "IMAGE OCR TEXT"


def test_run_ocr_reports_empty_result(tmp_path: Path):
    pdf_path = tmp_path / "scan.pdf"
    model_dir = tmp_path / "models"
    create_model_assets(model_dir)
    write_test_pdf(pdf_path)
    request = parse_request(
        json.dumps(
            {
                "filePath": str(pdf_path),
                "modelDir": str(model_dir),
                "tier": "medium",
            }
        )
    )

    response = run_ocr(request, ocr_factory=lambda _request: lambda _path: [])

    assert response["ok"] is False
    assert response["error"]["code"] == "OCR_EMPTY_RESULT"


def test_run_ocr_rejects_pdf_over_page_limit_before_engine(tmp_path: Path):
    pdf_path = tmp_path / "scan.pdf"
    model_dir = tmp_path / "models"
    create_model_assets(model_dir)
    write_test_pdf(pdf_path, page_count=2)
    request = parse_request(
        json.dumps(
            {
                "filePath": str(pdf_path),
                "modelDir": str(model_dir),
                "tier": "medium",
                "maxPdfPages": 1,
            }
        )
    )

    response = run_ocr(
        request,
        ocr_factory=lambda _request: (_ for _ in ()).throw(
            AssertionError("OCR engine should not run")
        ),
    )

    assert response["ok"] is False
    assert response["error"]["code"] == "OCR_TOO_MANY_PAGES"


def test_real_ocr_engine_forces_model_source_flags(monkeypatch, tmp_path: Path):
    monkeypatch.setenv("DISABLE_MODEL_SOURCE_CHECK", "False")
    monkeypatch.setenv("PADDLE_PDX_DISABLE_MODEL_SOURCE_CHECK", "False")
    fake_module = types.ModuleType("paddleocr")

    class FakePaddleOCR:
        def __init__(self, **_kwargs):
            pass

        def predict(self, _path: str):
            return []

    fake_module.PaddleOCR = FakePaddleOCR
    monkeypatch.setitem(sys.modules, "paddleocr", fake_module)
    request = parse_request(
        json.dumps(
            {
                "filePath": str(tmp_path / "scan.pdf"),
                "modelDir": str(tmp_path / "models"),
                "tier": "medium",
            }
        )
    )

    build_real_ocr_engine(request)

    assert os.environ["DISABLE_MODEL_SOURCE_CHECK"] == "True"
    assert os.environ["PADDLE_PDX_DISABLE_MODEL_SOURCE_CHECK"] == "True"


def create_model_assets(model_dir: Path) -> None:
    for model_name in ("PP-OCRv6_medium_det", "PP-OCRv6_medium_rec"):
        model_path = model_dir / model_name
        model_path.mkdir(parents=True)
        for file_name in ("inference.json", "inference.pdiparams", "inference.yml"):
            (model_path / file_name).write_text("model", encoding="utf-8")


def write_test_pdf(pdf_path: Path, page_count: int = 1) -> None:
    from pypdf import PdfWriter

    writer = PdfWriter()
    for _ in range(page_count):
        writer.add_blank_page(width=120, height=120)
    with pdf_path.open("wb") as file:
        writer.write(file)
