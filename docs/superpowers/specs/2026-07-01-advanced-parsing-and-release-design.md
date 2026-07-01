# Advanced Parsing and Release Pipeline Design

**Date:** 2026-07-01
**Status:** Draft

## 1. Context and Goals
Following the successful integration of the Vision Sidecar (Module 9), the system can now intelligently caption embedded images. The next major leap in knowledge base quality requires addressing the most difficult unstructured formats: complex PDFs and nested/spanning tables. Additionally, the application requires a robust update mechanism for client distribution.

The goal of this phase is to implement three consecutive modules:
- **Module 10:** PDF High-Fidelity Layout Analysis
- **Module 11:** Complex Table Reasoning
- **Module 12:** Auto-Updater and Code Signing

## 2. Architecture & Design

### Module 10: PDF High-Fidelity Layout Analysis
- **Problem:** Current PDF parsing (pypdf) reads text linearly, which completely breaks multi-column layouts, mixes headers/footers with body text, and loses reading order.
- **Approach:**
  - Enhance parser_sidecar.py to use a layout-aware PDF extraction library (e.g., pdfplumber or marker for local PDF-to-Markdown).
  - Implement a heuristic to filter out repetitive headers/footers based on bounding box coordinates and page frequency.
  - Expose layout-aware markdown segments back to the Rust side.
- **Constraints:** Must remain local-first. Models used for layout analysis must be lightweight enough to run without requiring a high-end GPU (similar to Moondream2's footprint).

### Module 11: Complex Table Reasoning
- **Problem:** Tables with spanned rows/columns are flattened poorly, destroying relational data context.
- **Approach:**
  - Utilize the layout analysis from Module 10 to identify table boundaries in PDFs.
  - Implement table extraction that converts grid structures into Markdown tables or HTML tables.
  - Update sqlite.rs and the search indexing to recognize 	able chunks.
  - During semantic retrieval, tables will be retrieved as intact blocks, rather than broken lines, ensuring the LLM sees the row/column context.
- **Dependencies:** Relies on the bounding-box and extraction capabilities introduced in Module 10.

### Module 12: Auto-Updater and Release Pipeline
- **Problem:** Users currently have to manually download new installers.
- **Approach:**
  - Enable Tauri's built-in updater (	auri.conf.json -> updater).
  - Configure a GitHub Actions workflow to generate and expose the .sig signatures.
  - Generate a public/private key pair via Tauri CLI, storing the private key as a GitHub Secret.
  - Publish latest.json alongside the GitHub Release artifacts so the app can poll it on startup.
- **Security:** The public key will be embedded in 	auri.conf.json so the client only accepts signed updates.

## 3. Data Flow

1. **PDF Ingestion:** File -> Rust Scanner -> parser_sidecar -> Layout Analysis (Headers stripped, columns merged properly, tables tagged) -> Markdown chunks -> Rust Vector/FTS Index.
2. **Updates:** App launch -> Tauri Updater checks https://github.com/SakuraCianna/Library/releases/latest/download/latest.json -> Prompts user -> Downloads .nsis.zip -> Verifies signature -> Installs.

## 4. Implementation Sequence
We will execute these strictly as separated modules to maintain the "Sustained Stability" rule. Module 10 first, then 11, then 12.

## 5. Open Questions
- For Module 10, is the user okay with adding pdfplumber (which relies on pdfminer.six) or do they prefer a vision-based layout model (like surya or marker) which might require downloading another 1-2GB model? (Recommendation: Start with pdfplumber for CPU efficiency).
