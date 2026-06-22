from __future__ import annotations

import argparse
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
from typing import Any


OCR_VERSION = "PP-OCRv6"
DEFAULT_MAX_IMAGE_PIXELS = 25_000_000
REQUIRED_MODEL_FILES = ("inference.json", "inference.pdiparams", "inference.yml")


def required_model_paths(model_dir: Path, tier: str) -> tuple[Path, Path]:
    return (
        model_dir / f"{OCR_VERSION}_{tier}_det",
        model_dir / f"{OCR_VERSION}_{tier}_rec",
    )


def missing_model_assets(model_dir: Path, tier: str) -> list[str]:
    missing: list[str] = []
    for model_path in required_model_paths(model_dir, tier):
        if not model_path.is_dir():
            missing.append(model_path.name)
            continue

        for file_name in REQUIRED_MODEL_FILES:
            if not (model_path / file_name).is_file():
                missing.append(f"{model_path.name}/{file_name}")

    return missing


def module_available(module_name: str) -> bool:
    return importlib.util.find_spec(module_name) is not None


def make_check(name: str, ok: bool, message: str, **details: Any) -> dict[str, Any]:
    check: dict[str, Any] = {"name": name, "ok": ok, "message": message}
    if details:
        check["details"] = details
    return check


def smoke_test_sidecar(
    *,
    sidecar_path: Path,
    smoke_pdf: Path,
    model_dir: Path,
    tier: str,
    max_pdf_pages: int,
    max_image_pixels: int,
    timeout_seconds: int,
) -> dict[str, Any]:
    if not smoke_pdf.is_file():
        return make_check("smoke", False, "smoke file not found", path=str(smoke_pdf))

    payload = {
        "filePath": str(smoke_pdf),
        "modelDir": str(model_dir),
        "tier": tier,
        "maxPdfPages": max_pdf_pages,
        "maxImagePixels": max_image_pixels,
    }

    try:
        completed = subprocess.run(
            [sys.executable, str(sidecar_path)],
            input=json.dumps(payload, ensure_ascii=False),
            text=True,
            capture_output=True,
            timeout=timeout_seconds,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return make_check("smoke", False, "OCR smoke timed out", timeoutSeconds=timeout_seconds)

    stdout = completed.stdout.strip()
    try:
        response = json.loads(stdout)
    except json.JSONDecodeError:
        return make_check(
            "smoke",
            False,
            "OCR sidecar did not return valid JSON",
            exitCode=completed.returncode,
            stdout=stdout[-500:],
            stderr=completed.stderr[-500:],
        )

    if completed.returncode != 0:
        return make_check(
            "smoke",
            False,
            "OCR sidecar process failed",
            exitCode=completed.returncode,
            response=response,
        )

    if not response.get("ok"):
        error = response.get("error") or {}
        return make_check(
            "smoke",
            False,
            f"OCR smoke failed: {error.get('code', 'UNKNOWN')}",
            response=response,
        )

    result = response.get("result") or {}
    return make_check(
        "smoke",
        True,
        "OCR smoke passed",
        pageCount=result.get("pageCount"),
        textLength=len(str(result.get("text") or "")),
    )


def build_report(
    *,
    model_dir: Path,
    tier: str,
    sidecar_path: Path,
    require_runtime: bool,
    smoke_pdf: Path | None,
    max_pdf_pages: int,
    max_image_pixels: int,
    timeout_seconds: int,
) -> dict[str, Any]:
    checks: list[dict[str, Any]] = []

    missing_assets = missing_model_assets(model_dir, tier)
    checks.append(
        make_check(
            "models",
            not missing_assets,
            "OCR model assets complete" if not missing_assets else "OCR model assets missing",
            modelDir=str(model_dir),
            tier=tier,
            missing=missing_assets,
        )
    )

    checks.append(
        make_check(
            "sidecar",
            sidecar_path.is_file(),
            "OCR sidecar file exists" if sidecar_path.is_file() else "OCR sidecar file missing",
            path=str(sidecar_path),
        )
    )

    checks.append(
        make_check(
            "pypdf",
            module_available("pypdf"),
            "pypdf installed" if module_available("pypdf") else "pypdf missing",
        )
    )

    if require_runtime:
        checks.append(
            make_check(
                "paddleocr",
                module_available("paddleocr"),
                "paddleocr installed" if module_available("paddleocr") else "paddleocr missing",
            )
        )
        checks.append(
            make_check(
                "paddlepaddle",
                module_available("paddle"),
                "paddlepaddle installed" if module_available("paddle") else "paddlepaddle missing",
            )
        )

    if smoke_pdf is not None:
        checks.append(
            smoke_test_sidecar(
                sidecar_path=sidecar_path,
                smoke_pdf=smoke_pdf,
                model_dir=model_dir,
                tier=tier,
                max_pdf_pages=max_pdf_pages,
                max_image_pixels=max_image_pixels,
                timeout_seconds=timeout_seconds,
            )
        )

    return {"ok": all(check["ok"] for check in checks), "checks": checks}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Check the local Library OCR environment")
    parser.add_argument("--model-dir", required=True)
    parser.add_argument("--tier", default="medium", choices=("tiny", "small", "medium"))
    parser.add_argument("--sidecar", default=str(Path(__file__).with_name("ocr_sidecar.py")))
    parser.add_argument("--require-runtime", action="store_true")
    parser.add_argument("--smoke-file")
    parser.add_argument("--smoke-pdf")
    parser.add_argument("--max-pdf-pages", type=int, default=12)
    parser.add_argument("--max-image-pixels", type=int, default=DEFAULT_MAX_IMAGE_PIXELS)
    parser.add_argument("--timeout-seconds", type=int, default=180)
    parser.add_argument("--json", action="store_true")
    return parser.parse_args()


def print_human(report: dict[str, Any]) -> None:
    for check in report["checks"]:
        prefix = "OK" if check["ok"] else "FAIL"
        print(f"[{prefix}] {check['name']}: {check['message']}")
        details = check.get("details") or {}
        missing = details.get("missing")
        if missing:
            print("  missing: " + ", ".join(str(item) for item in missing))


def main() -> int:
    args = parse_args()
    smoke_input = args.smoke_file or args.smoke_pdf
    report = build_report(
        model_dir=Path(args.model_dir),
        tier=args.tier,
        sidecar_path=Path(args.sidecar),
        require_runtime=args.require_runtime,
        smoke_pdf=Path(smoke_input) if smoke_input else None,
        max_pdf_pages=args.max_pdf_pages,
        max_image_pixels=args.max_image_pixels,
        timeout_seconds=args.timeout_seconds,
    )

    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2))
    else:
        print_human(report)

    return 0 if report["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
