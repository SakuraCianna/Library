from __future__ import annotations

import json
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable

SUPPORTED_EXTENSIONS = {".png", ".jpg", ".jpeg", ".bmp", ".webp"}
MAX_INPUT_BYTES = 50 * 1024 * 1024


@dataclass(frozen=True)
class VisionRequest:
    file_path: str
    model_dir: str
    prompt: str = "Describe this image in detail. If it contains a chart or diagram, explain the data and its meaning."


def parse_request(raw: str) -> VisionRequest:
    payload = json.loads(raw)
    return VisionRequest(
        file_path=str(payload["filePath"]),
        model_dir=str(payload["modelDir"]),
        prompt=str(payload.get("prompt", "Describe this image in detail.")),
    )


def build_error_response(code: str, message: str) -> dict[str, Any]:
    return {"ok": False, "error": {"code": code, "message": message}}


def build_success_response(caption: str) -> dict[str, Any]:
    return {
        "ok": True,
        "result": {
            "caption": caption,
        },
    }


def validate_request(request: VisionRequest) -> dict[str, Any] | None:
    file_path = Path(request.file_path)
    if not file_path.is_file():
        return build_error_response("INPUT_NOT_FOUND", "输入文件不存在")
    if file_path.stat().st_size > MAX_INPUT_BYTES:
        return build_error_response("VISION_INPUT_TOO_LARGE", "输入图片超过 50 MB")
    
    extension = file_path.suffix.lower()
    if extension not in SUPPORTED_EXTENSIONS:
        return build_error_response("VISION_UNSUPPORTED_FILE", "当前仅支持常见的图片格式")

    model_dir = Path(request.model_dir)
    if not model_dir.is_dir() or not (model_dir / "config.json").is_file():
        return build_error_response("VISION_MODEL_MISSING", "视觉模型目录不完整，找不到 config.json")

    return None


def run_vision(
    request: VisionRequest,
    model_factory: Callable[[str], Any] | None = None,
) -> dict[str, Any]:
    validation_error = validate_request(request)
    if validation_error is not None:
        return validation_error

    try:
        if model_factory:
            model, tokenizer, encode_image = model_factory(request.model_dir)
        else:
            import warnings
            warnings.filterwarnings("ignore")
            
            import torch
            from PIL import Image
            from transformers import AutoModelForCausalLM, AutoTokenizer

            model_dir = request.model_dir
            # Using CPU by default for safety on consumer hardware, can be optimized later
            device = "cuda" if torch.cuda.is_available() else "cpu"
            
            model = AutoModelForCausalLM.from_pretrained(
                model_dir, 
                trust_remote_code=True,
                local_files_only=True
            ).to(device)
            model.eval()
            
            tokenizer = AutoTokenizer.from_pretrained(
                model_dir,
                local_files_only=True
            )
            
            def encode_image(image_path: str) -> Any:
                img = Image.open(image_path).convert("RGB")
                return model.encode_image(img)

        file_path = request.file_path
        enc_image = encode_image(file_path)
        caption = model.answer_question(enc_image, request.prompt, tokenizer)
        
        # Some versions return a string directly, others might return a dict or generator
        if not isinstance(caption, str):
            caption = str(caption)
            
        caption = caption.strip()
        if not caption:
            return build_error_response("VISION_EMPTY_RESULT", "模型未返回任何描述文本")
            
        return build_success_response(caption)

    except Exception as exc:
        return build_error_response("VISION_RUNTIME_ERROR", str(exc))


def main() -> int:
    raw = sys.stdin.read()
    try:
        request = parse_request(raw)
        response = run_vision(request)
    except Exception as exc:
        response = build_error_response("VISION_SIDECAR_ERROR", str(exc))

    sys.stdout.write(json.dumps(response, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
