# Module 10: PDF High-Fidelity Layout Analysis Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (- [ ]) syntax for tracking.

**Goal:** Improve PDF text extraction by replacing pypdf with pdfplumber to handle multi-column layouts and crop out repetitive headers/footers.

**Architecture:** We will modify parser_sidecar.py to use pdfplumber. Before extracting text, we will crop the top 5% and bottom 5% of each page to heuristically strip headers and footers, then extract the layout-aware text.

**Tech Stack:** Python, pdfplumber

---

### Task 1: Update Requirements

**Files:**
- Modify: sidecars/requirements.txt

- [ ] **Step 1: Add pdfplumber**

Update sidecars/requirements.txt to include pdfplumber==0.11.0 (or similar recent version).
Wait, I don't need to specify the exact line, just run a command.

`ash
echo "pdfplumber==0.11.0" >> sidecars/requirements.txt
`

- [ ] **Step 2: Commit**

`ash
git add sidecars/requirements.txt
git commit -m "chore(parser): add pdfplumber to requirements"
`

### Task 2: Implement Layout-Aware PDF Extraction

**Files:**
- Modify: sidecars/parser/parser_sidecar.py

- [ ] **Step 1: Write minimal implementation**

Modify ead_pdf_text in sidecars/parser/parser_sidecar.py to use pdfplumber and crop headers/footers.

Find this block:
`python
    try:
        from pypdf import PdfReader

        reader = PdfReader(file_path)
        pages = [page.extract_text() or "" for page in reader.pages]
`

Replace it with:
`python
    try:
        import pdfplumber

        pages_text = []
        with pdfplumber.open(file_path) as pdf:
            for page in pdf.pages:
                width = page.width
                height = page.height
                # Crop top 5% and bottom 5% to remove headers/footers
                bbox = (0, height * 0.05, width, height * 0.95)
                try:
                    cropped_page = page.crop(bbox)
                    text = cropped_page.extract_text()
                except ValueError:
                    # Fallback if crop fails
                    text = page.extract_text()
                
                pages_text.append(text or "")
                
        page_segments = page_text_segments(relative_path, pages_text)
`
Wait, remove rom pypdf import PdfReader if it exists.

- [ ] **Step 2: Run test to verify it passes**

Since pp tests spawn the parser, they might fail if pdfplumber is missing in the test environment, so you might need to install it first:
`ash
pip install -r sidecars/requirements.txt
`
Then run the tests:
`ash
cd app
cargo test -p app -- test_parser_sidecar
`
Wait, the Rust tests are for the Rust side. The python tests are in sidecars/parser.
`ash
cd sidecars/parser
pytest test_parser_sidecar.py -v
`
Expected: PASS

- [ ] **Step 3: Commit**

`ash
git add sidecars/parser/parser_sidecar.py
git commit -m "feat(parser): use pdfplumber for high-fidelity layout and crop headers"
`

