import json

from ocr_sidecar import build_error_response, parse_request


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


def test_error_response_is_json_serializable():
    response = build_error_response("OCR_MODEL_MISSING", "模型目录不存在")

    assert response["ok"] is False
    assert response["error"]["code"] == "OCR_MODEL_MISSING"
    assert "模型目录不存在" in response["error"]["message"]
