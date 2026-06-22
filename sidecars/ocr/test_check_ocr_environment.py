from __future__ import annotations

from pathlib import Path

from check_ocr_environment import (
    build_report,
    missing_model_assets,
    module_available,
    smoke_test_sidecar,
)


def test_missing_model_assets_reports_model_dirs_and_files(tmp_path: Path):
    model_dir = tmp_path / "models"
    (model_dir / "PP-OCRv6_medium_det").mkdir(parents=True)
    (model_dir / "PP-OCRv6_medium_det" / "inference.json").write_text(
        "{}", encoding="utf-8"
    )

    missing = missing_model_assets(model_dir, "medium")

    assert "PP-OCRv6_medium_det/inference.pdiparams" in missing
    assert "PP-OCRv6_medium_det/inference.yml" in missing
    assert "PP-OCRv6_medium_rec" in missing


def test_build_report_passes_with_minimal_local_assets(tmp_path: Path):
    model_dir = create_model_assets(tmp_path / "models")
    sidecar = tmp_path / "ocr_sidecar.py"
    sidecar.write_text("print('{}')\n", encoding="utf-8")

    report = build_report(
        model_dir=model_dir,
        tier="medium",
        sidecar_path=sidecar,
        require_runtime=False,
        smoke_pdf=None,
        max_pdf_pages=12,
        timeout_seconds=10,
    )

    assert report["ok"]
    assert [check["name"] for check in report["checks"]] == [
        "models",
        "sidecar",
        "pypdf",
    ]


def test_module_available_handles_missing_module():
    assert module_available("sys")
    assert not module_available("library_ocr_missing_runtime_module")


def test_smoke_test_sidecar_runs_python_json_protocol(tmp_path: Path):
    sidecar = tmp_path / "fake_sidecar.py"
    sidecar.write_text(
        "\n".join(
            [
                "import json, sys",
                "json.loads(sys.stdin.read())",
                "print(json.dumps({'ok': True, 'result': {'text': 'OCR', 'pageCount': 1}}))",
            ]
        ),
        encoding="utf-8",
    )
    smoke_pdf = tmp_path / "smoke.pdf"
    smoke_pdf.write_bytes(b"%PDF-1.4\n%%EOF")

    check = smoke_test_sidecar(
        sidecar_path=sidecar,
        smoke_pdf=smoke_pdf,
        model_dir=tmp_path / "models",
        tier="medium",
        max_pdf_pages=12,
        timeout_seconds=10,
    )

    assert check["ok"]
    assert check["details"]["textLength"] == 3


def create_model_assets(model_dir: Path) -> Path:
    for model_name in ("PP-OCRv6_medium_det", "PP-OCRv6_medium_rec"):
        model_path = model_dir / model_name
        model_path.mkdir(parents=True)
        for file_name in ("inference.json", "inference.pdiparams", "inference.yml"):
            (model_path / file_name).write_text("model", encoding="utf-8")
    return model_dir
