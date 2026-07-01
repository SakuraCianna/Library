from __future__ import annotations

from dataclasses import dataclass
from html import unescape
import json
from pathlib import Path
import posixpath
import re
import sys
from typing import Any
import zipfile


SUPPORTED_EXTENSIONS = {".pdf", ".docx", ".xlsx", ".md", ".txt"}
DEFAULT_MAX_INPUT_BYTES = 50 * 1024 * 1024
MAX_ZIP_ENTRY_BYTES = 10 * 1024 * 1024
MAX_ZIP_TOTAL_BYTES = 30 * 1024 * 1024
MAX_BODY_CHARS = 60_000
SUMMARY_CHARS = 180
MAX_TABLE_CELL_CHARS = 80
TABLE_SAMPLE_ROWS = 3
ALLOWED_EVIDENCE_KINDS = {"pdf_page", "ocr_page", "table_section", "embedded_image"}
DOCX_IMAGE_EXTENSIONS = {
    ".bmp",
    ".emf",
    ".gif",
    ".jpeg",
    ".jpg",
    ".png",
    ".svg",
    ".tif",
    ".tiff",
    ".webp",
    ".wmf",
}


@dataclass(frozen=True)
class ParserRequest:
    file_path: str
    relative_path: str
    max_input_bytes: int = DEFAULT_MAX_INPUT_BYTES


class ParserInputTooLarge(ValueError):
    pass


@dataclass
class ZipReadBudget:
    used_bytes: int = 0

    def read_text(self, archive: zipfile.ZipFile, name: str) -> str:
        info = archive.getinfo(name)
        if info.file_size > MAX_ZIP_ENTRY_BYTES:
            raise ParserInputTooLarge(
                f"压缩文档内部 XML 超过 {MAX_ZIP_ENTRY_BYTES // 1024 // 1024} MB：{name}"
            )
        next_total = self.used_bytes + info.file_size
        if next_total > MAX_ZIP_TOTAL_BYTES:
            raise ParserInputTooLarge(
                f"压缩文档 XML 解压后超过 {MAX_ZIP_TOTAL_BYTES // 1024 // 1024} MB"
            )
        self.used_bytes = next_total
        return archive.read(info).decode("utf-8", errors="replace")

    def read_optional_text(self, archive: zipfile.ZipFile, name: str) -> str:
        try:
            return self.read_text(archive, name)
        except KeyError:
            return ""


def parse_request(raw: str) -> ParserRequest:
    payload = json.loads(raw)
    return ParserRequest(
        file_path=str(payload["filePath"]),
        relative_path=str(payload["relativePath"]),
        max_input_bytes=int(payload.get("maxInputBytes") or DEFAULT_MAX_INPUT_BYTES),
    )


def read_stdin_payload() -> str:
    stdin_buffer = getattr(sys.stdin, "buffer", None)
    if stdin_buffer is None:
        return sys.stdin.read()
    return stdin_buffer.read().decode("utf-8-sig")


def build_error_response(code: str, message: str) -> dict[str, Any]:
    return {"ok": False, "error": {"code": code, "message": message}}


def build_success_response(
    *,
    relative_path: str,
    body: str,
    segments: list[dict[str, Any]] | None = None,
    table_insights: list[dict[str, Any]] | None = None,
) -> dict[str, Any]:
    normalized_body = normalize_text(body)
    return {
        "ok": True,
        "result": {
            "title": display_file_name(relative_path),
            "body": truncate_chars(normalized_body, MAX_BODY_CHARS),
            "summary": truncate_chars(normalized_body, SUMMARY_CHARS),
            "sourceLocator": relative_path,
            "segments": normalize_segments(relative_path, segments or []),
            "tableInsights": table_insights or [],
        },
    }


def run_parse(request: ParserRequest) -> dict[str, Any]:
    file_path = Path(request.file_path)
    if not file_path.is_file():
        return build_error_response("PARSER_INPUT_NOT_FOUND", "输入文件不存在")
    if file_path.stat().st_size > request.max_input_bytes:
        return build_error_response("PARSER_INPUT_TOO_LARGE", "文档解析输入文件超过 50 MB")

    extension = file_path.suffix.lower()
    if extension not in SUPPORTED_EXTENSIONS:
        return build_error_response(
            "PARSER_UNSUPPORTED_FILE",
            "当前文档解析仅支持 PDF、DOCX、XLSX、Markdown 和 TXT 文件",
        )

    try:
        segments: list[dict[str, Any]] = []
        if extension in {".md", ".txt"}:
            body = read_text_lossy(file_path)
            table_insights: list[dict[str, Any]] = []
        elif extension == ".pdf":
            body, segments, table_insights = read_pdf_text(file_path, request.relative_path)
        elif extension == ".docx":
            body, segments = read_docx_analysis(file_path, request.relative_path)
            table_insights = []
        else:
            body, table_insights = read_xlsx_analysis(file_path, request.relative_path)
    except ParserInputTooLarge as exc:
        return build_error_response("PARSER_INPUT_TOO_LARGE", str(exc))
    except Exception as exc:
        return build_error_response("PARSER_RUNTIME_ERROR", str(exc))

    if not normalize_text(body):
        return build_error_response("PARSER_EMPTY_RESULT", "没有从文件中提取到可索引文本")

    return build_success_response(
        relative_path=request.relative_path,
        body=body,
        segments=segments,
        table_insights=table_insights,
    )


def read_text_lossy(file_path: Path) -> str:
    return file_path.read_bytes().decode("utf-8", errors="replace")


def format_markdown_table(table: list[list[str | None]]) -> str:
    if not table or not table[0]:
        return ""
    
    cleaned_table = []
    column_count = 0
    for row in table:
        cleaned_row = []
        for cell in row:
            if cell is None:
                cleaned_row.append("")
            else:
                cleaned_row.append(str(cell).replace("\n", " ").replace("|", "\\|"))
        column_count = max(column_count, len(cleaned_row))
        cleaned_table.append(cleaned_row)

    if column_count == 0:
        return ""

    lines = []
    header = cleaned_table[0]
    header += [""] * (column_count - len(header))
    lines.append("| " + " | ".join(header) + " |")
    lines.append("|" + "|".join(["---"] * column_count) + "|")
    
    for row in cleaned_table[1:]:
        row += [""] * (column_count - len(row))
        lines.append("| " + " | ".join(row) + " |")
        
    return "\n".join(lines)


def read_pdf_text(file_path: Path, relative_path: str) -> tuple[str, list[dict[str, Any]], list[dict[str, Any]]]:
    file_name = display_file_name(relative_path)
    try:
        import pdfplumber

        pages_text = []
        table_insights = []
        with pdfplumber.open(file_path) as pdf:
            for page_idx, page in enumerate(pdf.pages, start=1):
                width = page.width
                height = page.height
                
                tables = page.extract_tables()
                for table_idx, table in enumerate(tables, start=1):
                    if not table or not table[0]:
                        continue
                    row_count = len(table)
                    column_count = max(len(row) for row in table)
                    header_row = [str(c).replace("\n", " ") for c in table[0] if c]
                    header_summary = join_limited(header_row, "、", 12) if header_row else "未识别表头"
                    
                    markdown_table = format_markdown_table(table)
                    if not markdown_table:
                        continue
                        
                    table_insights.append({
                        "title": f"{file_name} · 第 {page_idx} 页 表格 {table_idx}",
                        "body": markdown_table,
                        "summary": f"PDF 表格（第 {page_idx} 页）：{row_count} 行、{column_count} 列；表头：{header_summary}",
                        "sourceLocator": f"{relative_path}#page-{page_idx:03}-table-{table_idx}"
                    })

                # Crop top 5% and bottom 5% to remove headers/footers
                bbox = (0, height * 0.05, width, height * 0.95)
                try:
                    cropped_page = page.crop(bbox)
                    text = cropped_page.extract_text()
                except ValueError:
                    text = page.extract_text()
                pages_text.append(text or "")

        page_segments = page_text_segments(relative_path, pages_text)
        text = "\n".join(segment["body"] for segment in page_segments)
        if normalize_text(text):
            return text, page_segments, table_insights
    except Exception:
        pass

    content = file_path.read_bytes().decode("latin-1", errors="ignore")
    literal_text = extract_pdf_literal_strings(content)
    if len(literal_text.strip()) >= 4:
        return literal_text, page_text_segments(relative_path, [literal_text]), []
    readable_runs = extract_readable_runs(content)
    return readable_runs, page_text_segments(relative_path, [readable_runs]), []


def page_text_segments(relative_path: str, pages: list[str]) -> list[dict[str, Any]]:
    file_name = display_file_name(relative_path)
    segments = []
    for index, page in enumerate(pages, start=1):
        body = normalize_text(page)
        if not body:
            continue
        segments.append(
            {
                "title": f"{file_name} · 第 {index} 页",
                "body": body,
                "sourceLocator": f"{relative_path}#page-{index:03}",
                "evidence": {
                    "kind": "pdf_page",
                    "pageNumber": index,
                    "pageCount": len(pages),
                    "lineCount": line_count(page),
                    "charCount": len(body),
                },
            }
        )
    return segments


def normalize_segments(
    relative_path: str,
    segments: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    normalized = []
    file_name = display_file_name(relative_path)
    for index, segment in enumerate(segments, start=1):
        body = normalize_text(str(segment.get("body") or ""))
        if not body:
            continue
        title = normalize_text(str(segment.get("title") or "")) or f"{file_name} · 第 {index} 页"
        source_locator = normalize_text(str(segment.get("sourceLocator") or ""))
        if not source_locator:
            source_locator = f"{relative_path}#page-{index:03}"
        normalized_segment: dict[str, Any] = {
            "title": title,
            "body": truncate_chars(body, MAX_BODY_CHARS),
            "sourceLocator": source_locator,
        }
        evidence = normalize_evidence(segment.get("evidence"))
        if evidence is not None:
            normalized_segment["evidence"] = evidence
        normalized.append(normalized_segment)
    return normalized


def normalize_evidence(value: Any) -> dict[str, Any] | None:
    if not isinstance(value, dict):
        return None

    normalized: dict[str, Any] = {}
    kind = normalize_text(str(value.get("kind") or ""))
    if kind in ALLOWED_EVIDENCE_KINDS:
        normalized["kind"] = kind

    for key in (
        "pageNumber",
        "pageCount",
        "imageNumber",
        "lineCount",
        "charCount",
        "confidencePercent",
    ):
        number = bounded_positive_int(value.get(key))
        if number is not None:
            normalized[key] = number

    return normalized or None


def bounded_positive_int(value: Any) -> int | None:
    try:
        number = int(value)
    except (TypeError, ValueError):
        return None
    if number <= 0:
        return None
    return min(number, 1_000_000)


def read_docx_text(file_path: Path) -> str:
    body, _segments = read_docx_analysis(file_path, str(file_path.name))
    return body


def read_docx_analysis(file_path: Path, relative_path: str) -> tuple[str, list[dict[str, Any]]]:
    with zipfile.ZipFile(file_path) as archive:
        budget = ZipReadBudget()
        document = budget.read_text(archive, "word/document.xml")
        relationships = budget.read_optional_text(archive, "word/_rels/document.xml.rels")
        document_text = xml_to_text(document)
        image_segments = docx_embedded_image_segments(
            relative_path,
            archive,
            document,
            relationships,
        )

    body_parts = []
    segments = []
    if document_text:
        body_parts.append(document_text)
        if image_segments:
            segments.append(
                {
                    "title": display_file_name(relative_path),
                    "body": document_text,
                    "sourceLocator": relative_path,
                }
            )
    for segment in image_segments:
        body_parts.append(segment["body"])
        segments.append(segment)

    return "\n".join(body_parts), segments


def docx_embedded_image_segments(
    relative_path: str,
    archive: zipfile.ZipFile,
    document_xml: str,
    relationships_xml: str,
) -> list[dict[str, Any]]:
    archive_names = set(archive.namelist())
    image_entries = docx_document_image_entries(document_xml, relationships_xml, archive_names)
    if not image_entries:
        alt_texts = docx_image_alt_texts(document_xml)
        image_names = [
            name
            for name in archive_names
            if name.startswith("word/media/")
            and not name.endswith("/")
            and display_file_name(name)
            and Path(name).suffix.lower() in DOCX_IMAGE_EXTENSIONS
        ]
        image_names.sort()
        image_entries = [
            {
                "target": image_name,
                "altText": alt_texts[index] if index < len(alt_texts) else None,
            }
            for index, image_name in enumerate(image_names)
        ]

    if not image_entries:
        return []

    file_name = display_file_name(relative_path)
    segments = []
    for index, image_entry in enumerate(image_entries, start=1):
        image_file_name = truncate_chars(display_file_name(image_entry["target"]), 120)
        source_locator = f"{relative_path}#image-{index:03}"
        lines = [
            f"{file_name} · 文档图片 {index}",
            f"来源：{source_locator}",
            f"图片文件：{image_file_name}",
            "说明：当前仅登记文档内图片和可用替代文本；未进行图片语义理解或 OCR。",
        ]
        if image_entry.get("altText"):
            lines.append(f"替代文本：{image_entry['altText']}")
        body = "\n".join(lines)
        normalized_body = normalize_text(body)
        segments.append(
            {
                "title": f"{file_name} · 文档图片 {index}",
                "body": body,
                "sourceLocator": source_locator,
                "evidence": {
                    "kind": "embedded_image",
                    "imageNumber": index,
                    "lineCount": line_count(body),
                    "charCount": len(normalized_body),
                },
            }
        )
    return segments


def docx_document_image_entries(
    document_xml: str,
    relationships_xml: str,
    archive_names: set[str],
) -> list[dict[str, str | None]]:
    relationships = docx_image_relationship_targets(relationships_xml, archive_names)
    if not relationships:
        return []

    entries = []
    for drawing_xml in extract_prefixed_xml_blocks(document_xml, "drawing"):
        alt_text = first_doc_pr_alt_text(drawing_xml)
        for blip_tag in re.findall(
            r"<(?:[A-Za-z0-9_]+:)?blip\b[^>]*>",
            drawing_xml,
            flags=re.S,
        ):
            relationship_id = (
                xml_attribute(blip_tag, "r:embed")
                or xml_attribute(blip_tag, "embed")
                or xml_attribute(blip_tag, "r:link")
                or xml_attribute(blip_tag, "link")
            )
            target = relationships.get(relationship_id or "")
            if target:
                entries.append({"target": target, "altText": alt_text})
    return entries


def docx_image_relationship_targets(
    relationships_xml: str,
    archive_names: set[str],
) -> dict[str, str]:
    targets = {}
    for match in re.finditer(r"<(?:[A-Za-z0-9_]+:)?Relationship\b[^>]*/?>", relationships_xml):
        tag = match.group(0)
        relationship_id = xml_attribute(tag, "Id")
        target = normalize_docx_relationship_target(xml_attribute(tag, "Target") or "")
        relationship_type = normalize_text(xml_attribute(tag, "Type") or "")
        if (
            relationship_id
            and target in archive_names
            and target.startswith("word/media/")
            and relationship_type.endswith("/image")
            and Path(target).suffix.lower() in DOCX_IMAGE_EXTENSIONS
        ):
            targets[relationship_id] = target
    return targets


def normalize_docx_relationship_target(target: str) -> str:
    normalized = unescape(target).strip().replace("\\", "/")
    if not normalized or re.match(r"^[A-Za-z][A-Za-z0-9+.-]*:", normalized):
        return ""
    if normalized.startswith("/"):
        return posixpath.normpath(normalized.lstrip("/"))
    return posixpath.normpath(posixpath.join("word", normalized))


def docx_image_alt_texts(document_xml: str) -> list[str]:
    alt_texts = []
    for match in re.finditer(r"<(?:[A-Za-z0-9_]+:)?docPr\b[^>]*>", document_xml):
        tag = match.group(0)
        for attribute in ("descr", "title", "name"):
            value = xml_attribute(tag, attribute)
            normalized = normalize_text(value or "")
            if normalized:
                alt_texts.append(truncate_chars(normalized, 300))
                break
    return alt_texts


def first_doc_pr_alt_text(xml: str) -> str | None:
    match = re.search(r"<(?:[A-Za-z0-9_]+:)?docPr\b[^>]*>", xml)
    if not match:
        return None
    tag = match.group(0)
    for attribute in ("descr", "title", "name"):
        value = xml_attribute(tag, attribute)
        normalized = normalize_text(value or "")
        if normalized:
            return truncate_chars(normalized, 300)
    return None


def read_xlsx_analysis(
    file_path: Path,
    relative_path: str,
) -> tuple[str, list[dict[str, Any]]]:
    with zipfile.ZipFile(file_path) as archive:
        budget = ZipReadBudget()
        names = archive.namelist()
        shared_strings_xml = ""
        sheet_parts: list[tuple[str, str]] = []
        for name in names:
            if name == "xl/sharedStrings.xml":
                shared_strings_xml = budget.read_text(archive, name)
            elif name.startswith("xl/worksheets/") and name.endswith(".xml"):
                sheet_parts.append((name, budget.read_text(archive, name)))

    sheet_parts.sort(key=lambda item: worksheet_sort_key(item[0]))
    shared_strings = parse_shared_strings(shared_strings_xml)
    body_parts = []
    table_insights = []

    shared_text = xml_to_text(shared_strings_xml)
    if shared_text:
        body_parts.append(shared_text)

    for index, (_name, xml) in enumerate(sheet_parts, start=1):
        text = xml_to_text(xml)
        if text:
            body_parts.append(text)
        insight = worksheet_table_insight(relative_path, index, xml, shared_strings)
        if insight:
            body_parts.append(insight["body"])
            table_insights.append(insight)

    return "\n".join(body_parts), table_insights


def worksheet_sort_key(name: str) -> int:
    file_name = name.rsplit("/", 1)[-1]
    if file_name.startswith("sheet") and file_name.endswith(".xml"):
        try:
            return int(file_name.removeprefix("sheet").removesuffix(".xml"))
        except ValueError:
            return 1_000_000
    return 1_000_000


def parse_shared_strings(xml: str) -> list[str]:
    return [xml_to_text(block) for block in extract_xml_blocks(xml, "si")]


def worksheet_table_insight(
    relative_path: str,
    sheet_index: int,
    xml: str,
    shared_strings: list[str],
) -> dict[str, Any] | None:
    row_count = 0
    column_count = 0
    header_values: list[str] = []
    sample_rows: list[list[str]] = []

    for row_xml in extract_xml_blocks(xml, "row"):
        values, row_columns = worksheet_row_summary(row_xml, shared_strings)
        if not values:
            continue
        row_count += 1
        column_count = max(column_count, row_columns)
        if row_count == 1:
            header_values = values
        elif len(sample_rows) < TABLE_SAMPLE_ROWS:
            sample_rows.append(values)

    if row_count == 0:
        return None

    header_summary = join_limited(header_values, "、", 12) if header_values else "未识别表头"
    title = f"{display_file_name(relative_path)} · 工作表 {sheet_index}"
    source_locator = f"{relative_path}#sheet-{sheet_index:03}"
    lines = [
        title,
        f"来源：{source_locator}",
        f"结构：{row_count} 行，{column_count} 列",
        f"表头：{header_summary}",
    ]
    for index, row in enumerate(sample_rows, start=1):
        sample = row_sample(row)
        if sample:
            lines.append(f"样例 {index}：{sample}")
    if header_values:
        lines.append(f"可问答字段：{header_summary}")

    body = "\n".join(lines)
    return {
        "title": title,
        "body": body,
        "summary": f"工作表 {sheet_index}：{row_count} 行、{column_count} 列；表头：{header_summary}",
        "sourceLocator": source_locator,
    }


def worksheet_row_summary(
    row_xml: str,
    shared_strings: list[str],
) -> tuple[list[str], int]:
    values = []
    column_count = 0
    fallback_column_index = 0
    for cell_xml in extract_xml_blocks(row_xml, "c"):
        column_index = cell_column_index(cell_xml)
        if column_index is None:
            column_index = fallback_column_index
        fallback_column_index = column_index + 1
        value = worksheet_cell_value(cell_xml, shared_strings)
        if value is not None:
            column_count = max(column_count, column_index + 1)
            values.append(value)
    return values, column_count


def worksheet_cell_value(cell_xml: str, shared_strings: list[str]) -> str | None:
    cell_type = xml_attribute(cell_xml, "t")
    value = None
    if cell_type == "s":
        raw_index = first_xml_tag_text(cell_xml, "v")
        if raw_index and raw_index.isdigit():
            index = int(raw_index)
            if 0 <= index < len(shared_strings):
                value = shared_strings[index]
    elif cell_type == "inlineStr":
        value = first_xml_tag_text(cell_xml, "is")
    else:
        value = first_xml_tag_text(cell_xml, "v")

    if value is None:
        return None
    normalized = normalize_text(value)
    if not normalized:
        return None
    return truncate_chars(normalized, MAX_TABLE_CELL_CHARS)


def xml_attribute(xml: str, attribute: str) -> str | None:
    match = re.search(rf'{re.escape(attribute)}="([^"]*)"', xml)
    return unescape(match.group(1)) if match else None


def first_xml_tag_text(xml: str, tag: str) -> str | None:
    match = re.search(rf"<{tag}(?:\s[^>]*)?>(.*?)</{tag}>", xml, flags=re.S)
    return xml_to_text(match.group(1)) if match else None


def cell_column_index(cell_xml: str) -> int | None:
    reference = xml_attribute(cell_xml, "r")
    if not reference:
        return None
    letters = "".join(character for character in reference if character.isalpha())
    if not letters:
        return None
    index = 0
    for character in letters.upper():
        if not ("A" <= character <= "Z"):
            return None
        index = index * 26 + (ord(character) - ord("A") + 1)
    return index - 1


def extract_xml_blocks(xml: str, tag: str) -> list[str]:
    return re.findall(rf"<{tag}(?:\s[^>]*)?>.*?</{tag}>", xml, flags=re.S)


def extract_prefixed_xml_blocks(xml: str, tag: str) -> list[str]:
    return re.findall(
        rf"<(?:[A-Za-z0-9_]+:)?{tag}(?:\s[^>]*)?>.*?</(?:[A-Za-z0-9_]+:)?{tag}>",
        xml,
        flags=re.S,
    )


def extract_pdf_literal_strings(content: str) -> str:
    values = []
    current = []
    in_string = False
    escaped = False
    for character in content:
        if in_string:
            if escaped:
                current.append(character)
                escaped = False
            elif character == "\\":
                escaped = True
            elif character == ")":
                value = "".join(current).strip()
                if len(value) >= 2:
                    values.append(value)
                current = []
                in_string = False
            else:
                current.append(character)
        elif character == "(":
            in_string = True
    return "\n".join(values)


def extract_readable_runs(content: str) -> str:
    runs = []
    current = []
    for character in content:
        if is_readable_character(character):
            current.append(character)
        else:
            push_readable_run(runs, current)
            current = []
    push_readable_run(runs, current)
    return "\n".join(runs)


def push_readable_run(runs: list[str], current: list[str]) -> None:
    value = normalize_text("".join(current))
    if len(value) >= 4:
        runs.append(value)


def is_readable_character(character: str) -> bool:
    return (
        character.isalnum()
        or character.isspace()
        or character in "，。、；：？！,.;:?!-_\\/()"
    )


def row_sample(row: list[str]) -> str:
    return join_limited([value for value in row if value.strip()], " | ", 8)


def join_limited(values: list[str], separator: str, limit: int) -> str:
    output = values[:limit]
    if len(values) > limit:
        output.append(f"另有 {len(values) - limit} 项")
    return separator.join(output)


def xml_to_text(xml: str) -> str:
    without_tags = re.sub(r"<[^>]+>", " ", xml)
    return normalize_text(unescape(without_tags))


def normalize_text(value: str) -> str:
    return " ".join(value.split())


def line_count(value: str) -> int:
    count = sum(1 for line in value.splitlines() if line.strip())
    return max(1, count)


def truncate_chars(value: str, max_chars: int) -> str:
    if len(value) <= max_chars:
        return value
    return value[:max_chars] + "…"


def display_file_name(relative_path: str) -> str:
    normalized = relative_path.replace("/", "\\")
    return normalized.rsplit("\\", 1)[-1] or relative_path


def main() -> int:
    raw = read_stdin_payload()
    try:
        request = parse_request(raw)
        response = run_parse(request)
    except Exception as exc:
        response = build_error_response("PARSER_SIDECAR_ERROR", str(exc))

    sys.stdout.write(json.dumps(response, ensure_ascii=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
