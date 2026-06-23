import io
import json
from pathlib import Path
import zipfile

import parser_sidecar
from parser_sidecar import parse_request, run_parse, main


def test_parse_request_accepts_local_file_and_relative_path(tmp_path: Path):
    file_path = tmp_path / "Redis.md"
    request = parse_request(
        json.dumps(
            {
                "filePath": str(file_path),
                "relativePath": "docs\\Redis.md",
                "maxInputBytes": 1024,
            }
        )
    )

    assert request.file_path == str(file_path)
    assert request.relative_path == "docs\\Redis.md"
    assert request.max_input_bytes == 1024


def test_run_parse_reads_markdown(tmp_path: Path):
    file_path = tmp_path / "Redis.md"
    file_path.write_text("# Redis\n\n缓存穿透需要空值缓存。", encoding="utf-8")

    response = run_parse(
        parse_request(
            json.dumps(
                {"filePath": str(file_path), "relativePath": "Redis.md"}
            )
        )
    )

    assert response["ok"] is True
    assert response["result"]["title"] == "Redis.md"
    assert "缓存穿透" in response["result"]["body"]
    assert response["result"]["sourceLocator"] == "Redis.md"


def test_main_reads_utf8_sig_payload_with_non_ascii_path(tmp_path: Path, capsys, monkeypatch):
    file_path = tmp_path / "知识库.md"
    file_path.write_text("# Redis\n\n缓存穿透需要空值缓存。", encoding="utf-8")
    payload = json.dumps(
        {"filePath": str(file_path), "relativePath": "知识库.md"},
        ensure_ascii=False,
    ).encode("utf-8-sig")

    monkeypatch.setattr("sys.stdin", type("Input", (), {"buffer": io.BytesIO(payload)})())

    exit_code = main()
    output = json.loads(capsys.readouterr().out)

    assert exit_code == 0
    assert output["ok"] is True
    assert "缓存穿透" in output["result"]["body"]


def test_run_parse_extracts_text_pdf_fallback(tmp_path: Path):
    file_path = tmp_path / "note.pdf"
    file_path.write_bytes(
        b"%PDF-1.4\n1 0 obj <<>> endobj\nBT (PDF cache penetration note) Tj ET\n%%EOF"
    )

    response = run_parse(
        parse_request(
            json.dumps(
                {"filePath": str(file_path), "relativePath": "note.pdf"}
            )
        )
    )

    assert response["ok"] is True
    assert "PDF cache penetration note" in response["result"]["body"]
    assert response["result"]["segments"] == [
        {
            "title": "note.pdf · 第 1 页",
            "body": "PDF cache penetration note",
            "sourceLocator": "note.pdf#page-001",
        }
    ]


def test_run_parse_extracts_docx_text(tmp_path: Path):
    file_path = tmp_path / "面试.docx"
    write_docx(file_path, "文档解析 Sidecar")

    response = run_parse(
        parse_request(
            json.dumps(
                {"filePath": str(file_path), "relativePath": "面试.docx"}
            )
        )
    )

    assert response["ok"] is True
    assert "文档解析 Sidecar" in response["result"]["body"]


def test_run_parse_extracts_xlsx_table_insight(tmp_path: Path):
    file_path = tmp_path / "经营报表.xlsx"
    write_xlsx(file_path)

    response = run_parse(
        parse_request(
            json.dumps(
                {"filePath": str(file_path), "relativePath": "经营报表.xlsx"}
            )
        )
    )

    assert response["ok"] is True
    result = response["result"]
    assert "经营报表.xlsx · 工作表 1" in result["body"]
    assert result["tableInsights"][0]["sourceLocator"] == "经营报表.xlsx#sheet-001"
    assert "月份、营收、成本" in result["tableInsights"][0]["summary"]


def test_run_parse_rejects_docx_zip_entry_over_uncompressed_limit(
    tmp_path: Path, monkeypatch
):
    monkeypatch.setattr(parser_sidecar, "MAX_ZIP_ENTRY_BYTES", 32)
    file_path = tmp_path / "large.docx"
    write_docx(file_path, "x" * 80)

    response = run_parse(
        parse_request(
            json.dumps({"filePath": str(file_path), "relativePath": "large.docx"})
        )
    )

    assert response["ok"] is False
    assert response["error"]["code"] == "PARSER_INPUT_TOO_LARGE"


def test_run_parse_rejects_xlsx_total_uncompressed_limit(tmp_path: Path, monkeypatch):
    monkeypatch.setattr(parser_sidecar, "MAX_ZIP_ENTRY_BYTES", 1024)
    monkeypatch.setattr(parser_sidecar, "MAX_ZIP_TOTAL_BYTES", 160)
    file_path = tmp_path / "large.xlsx"
    write_xlsx_with_large_xml_parts(file_path)

    response = run_parse(
        parse_request(
            json.dumps({"filePath": str(file_path), "relativePath": "large.xlsx"})
        )
    )

    assert response["ok"] is False
    assert response["error"]["code"] == "PARSER_INPUT_TOO_LARGE"


def test_run_parse_rejects_unsupported_file(tmp_path: Path):
    file_path = tmp_path / "archive.zip"
    file_path.write_text("zip", encoding="utf-8")

    response = run_parse(
        parse_request(
            json.dumps(
                {"filePath": str(file_path), "relativePath": "archive.zip"}
            )
        )
    )

    assert response["ok"] is False
    assert response["error"]["code"] == "PARSER_UNSUPPORTED_FILE"


def test_main_reports_malformed_request(capsys, monkeypatch):
    monkeypatch.setattr("sys.stdin", type("Input", (), {"read": lambda self: "not-json"})())

    exit_code = main()
    output = json.loads(capsys.readouterr().out)

    assert exit_code == 0
    assert output["ok"] is False
    assert output["error"]["code"] == "PARSER_SIDECAR_ERROR"


def write_docx(path: Path, text: str) -> None:
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr(
            "word/document.xml",
            f"""<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:body>
</w:document>""",
        )


def write_xlsx(path: Path) -> None:
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr(
            "xl/sharedStrings.xml",
            """<?xml version="1.0" encoding="UTF-8"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <si><t></t></si>
  <si><t>月份</t></si>
  <si><t>营收</t></si>
  <si><t>成本</t></si>
  <si><t>2026-06</t></si>
</sst>""",
        )
        archive.writestr(
            "xl/worksheets/sheet1.xml",
            """<?xml version="1.0" encoding="UTF-8"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData>
    <row r="1"><c r="A1" t="s"><v>1</v></c><c r="B1" t="s"><v>2</v></c><c r="C1" t="s"><v>3</v></c></row>
    <row r="2"><c r="A2" t="s"><v>4</v></c><c r="B2"><v>120</v></c><c r="C2"><v>70</v></c></row>
  </sheetData>
</worksheet>""",
        )


def write_xlsx_with_large_xml_parts(path: Path) -> None:
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr(
            "xl/sharedStrings.xml",
            "<sst>" + ("a" * 90) + "</sst>",
        )
        archive.writestr(
            "xl/worksheets/sheet1.xml",
            "<worksheet>" + ("b" * 90) + "</worksheet>",
        )
