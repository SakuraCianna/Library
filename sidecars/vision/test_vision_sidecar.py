import json
from pathlib import Path

from vision_sidecar import (
    VisionRequest,
    build_error_response,
    build_success_response,
    parse_request,
    run_vision,
    validate_request,
)


def test_parse_request():
    raw = json.dumps(
        {
            "filePath": "/test/image.jpg",
            "modelDir": "/models/vision",
            "prompt": "Test prompt",
        }
    )
    request = parse_request(raw)
    assert request.file_path == "/test/image.jpg"
    assert request.model_dir == "/models/vision"
    assert request.prompt == "Test prompt"


def test_validate_missing_file(tmp_path: Path):
    request = VisionRequest(
        file_path=str(tmp_path / "missing.jpg"),
        model_dir=str(tmp_path),
    )
    error = validate_request(request)
    assert error is not None
    assert error["error"]["code"] == "INPUT_NOT_FOUND"


def test_validate_missing_model(tmp_path: Path):
    img_path = tmp_path / "test.jpg"
    img_path.write_bytes(b"test")
    
    request = VisionRequest(
        file_path=str(img_path),
        model_dir=str(tmp_path / "missing_model"),
    )
    error = validate_request(request)
    assert error is not None
    assert error["error"]["code"] == "VISION_MODEL_MISSING"


def test_run_vision_success(tmp_path: Path):
    img_path = tmp_path / "test.jpg"
    img_path.write_bytes(b"test")
    
    model_dir = tmp_path / "model"
    model_dir.mkdir()
    (model_dir / "config.json").write_text("{}")
    
    request = VisionRequest(
        file_path=str(img_path),
        model_dir=str(model_dir),
        prompt="Describe it",
    )
    
    class MockModel:
        def answer_question(self, enc_image, prompt, tokenizer):
            return "This is a test caption."
            
    def mock_factory(model_dir):
        return MockModel(), "mock_tokenizer", lambda p: "enc_image"
        
    response = run_vision(request, model_factory=mock_factory)
    assert response["ok"] is True
    assert response["result"]["caption"] == "This is a test caption."
