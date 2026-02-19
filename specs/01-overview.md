# 01 — Overview & Goals

> **See also**: [Index](./00-index.md) · [Algorithm](./02-algorithm.md) · [API Design](./06-api-design.md)

---

## 1. Problem Statement

PDF is the de-facto standard for structured documents — academic papers, invoices, contracts, technical manuals — yet its content is essentially locked inside an opaque binary format that is hostile to downstream processing (search, LLM ingestion, embedding pipelines, version control).

Traditional approaches to PDF text extraction (`pdftotext`, `PyMuPDF`, `lopdf`) suffer from predictable, fundamental failures:

| Failure Mode | Root Cause |
|---|---|
| Text-order corruption | PDF renders via absolute coordinates; no logical reading-order guarantee |
| Table destruction | Tables are drawn via line objects, not structured data |
| Missing text | Text may be embedded as a bitmap (scanned documents) |
| Ligature/glyph loss | Custom CID font encodings map code points to private-use areas |
| Layout ambiguity | Multi-column, side-bars, footnotes interleave reading order |

### The Vision-LLM Approach

[Quantalogic PyZeroX](https://github.com/quantalogic/quantalogic-pyzerox) (and its upstream [ZeroX](https://github.com/getomni-ai/zerox)) proved that a fundamentally different strategy produces dramatically better results:

```
PDF page  ──rasterise──►  high-res bitmap  ──vision LLM──►  Markdown
```

Because the VLM (Vision Language Model) *sees* the page the same way a human does — as a rendered image — it correctly understands columns, tables, formulas, captions, sidebars, and even decorative text, producing clean, structured Markdown.

---

## 2. Mission

Build **`edgequake-pdf2md`** — a Rust crate that:

1. Accepts a **PDF input** (local file path or remote URL)
2. **Rasterises** each page to a high-resolution image via `pdfium-render`
3. Sends each page image to a **Vision LLM** via `edgequake-llm` multi-provider bindings
4. Aggregates and **post-processes** the per-page Markdown into a single coherent document
5. Writes the result to a **destination** (stdout, file path, or in-memory `String`)
6. Exposes both a **library API** (ergonomic Rust types) and a **CLI binary** (`pdf2md`)

---

## 3. Design Principles (First Principles)

### 3.1 Correctness over speed
PDF conversion is a lossy process. We optimise for maximum content fidelity over throughput. Speed is achieved via concurrency, not by skipping content.

### 3.2 Provider agnosticism
No part of the core pipeline assumes a specific LLM vendor. The `edgequake-llm` trait abstraction (`LLMProvider`) is the only coupling point. Swapping OpenAI for Ollama requires changing one environment variable.

### 3.3 Fail loudly, recover gracefully
Individual page failures should not abort the entire document. Each page is an independent unit of work. A per-page error produces a `PageResult::Error(...)` entry rather than panicking.

### 3.4 Zero unsafe code (except FFI boundary)
The `pdfium-render` crate wraps the Pdfium C++ library. All unsafe is confined to that crate. Our crate contains zero `unsafe` blocks.

### 3.5 Predictable resource usage
Large PDFs (300+ pages) must not buffer the entire document in memory. Pages are processed as a stream; only `concurrency_limit` pages are in-flight simultaneously.

### 3.6 CLI ergonomics first
The CLI must be intuitive enough that a first-time user can convert a PDF without reading documentation. Defaults should be sensible.

---

## 4. Scope

### In scope
- PDF files (any valid PDF 1.0–2.0 document)
- Remote PDF via HTTP/HTTPS URL
- Vision LLM providers supported by `edgequake-llm`: OpenAI, Anthropic, Azure OpenAI, Google Gemini, Ollama, LM Studio, OpenRouter, xAI
- Async library API (`async fn`)
- Synchronous convenience wrappers (`fn` blocking variants)
- CLI binary (`pdf2md`)
- Configurable rendering DPI (default 150; 300 for high-fidelity)
- Page selection (single page, range, arbitrary set)
- Streaming output (emit Markdown as pages complete, not all at once)
- Progress reporting (page count, elapsed time, token usage)

### Out of scope (v1.0)
- Non-PDF input formats (DOCX, PPTX, etc.) — these can be pre-converted to PDF
- PDF form filling or PDF writing
- Embedded document signatures validation
- PDF/A archival compliance checking
- WASM target (deferred; `pdfium-render` supports WASM but requires separate WASM build of Pdfium)

---

## 5. Success Criteria

| Criterion | Target |
|---|---|
| Accuracy (header/text/list) | ≥ 95% structural match on reference corpus |
| Table reconstruction | ≥ 80% of simple tables (≤ 10 cols) correctly formatted |
| Throughput (8-core machine, GPT-4o) | ≥ 5 pages/sec end-to-end at `concurrency=10` |
| Memory ceiling (300-page doc) | ≤ 500 MB peak RSS |
| CLI first-run time (simple doc) | < 30 seconds for a 10-page document |

---

## 6. Key Stakeholders & Usage Patterns

### Pattern A — Single document conversion (CLI)
```
pdf2md input.pdf -o output.md
```

### Pattern B — Batch pipeline (library)
```rust
let output = pdf2md::convert("report.pdf", &config).await?;
fs::write("report.md", output.markdown).await?;
```

### Pattern C — URL ingestion in a document processing service
```rust
let output = pdf2md::convert_url("https://arxiv.org/pdf/2310.12345", &config).await?;
```

### Pattern D — Streaming for real-time display
```rust
let mut stream = pdf2md::convert_stream("paper.pdf", &config).await?;
while let Some(page) = stream.next().await {
    println!("Page {}: {}", page.page_num, page.markdown);
}
```

---

## 7. Architecture Layers

```
+------------------------------------------------------------------+
|                        User Interface                            |
|            CLI (clap)             Library API (pub)              |
+----------------------------------+-------------------------------+
|                   Orchestration Layer                            |
|   Download  |  Page Selection  |  Concurrency Pool  |  Output   |
+------------------------------------------------------------------+
|                    Processing Pipeline                           |
|   PDF Load  |  Rasterise  |  Encode  |  VLM Call  |  Stitch   |
+------------------------------------------------------------------+
|                       Integration Layer                          |
|   pdfium-render (PDF→bitmap)   edgequake-llm (VLM providers)    |
+------------------------------------------------------------------+
|                       Infrastructure                             |
|   tokio (async)  reqwest (HTTP)  base64  image  tracing         |
+------------------------------------------------------------------+
```

---

## 8. Reference Implementations

| Project | Language | Strategy | Reference |
|---------|----------|----------|-----------|
| PyZeroX | Python | Poppler → image → litellm | [GitHub](https://github.com/quantalogic/quantalogic-pyzerox) |
| ZeroX (original) | TypeScript | Ghostscript → image → OpenAI | [GitHub](https://github.com/getomni-ai/zerox) |
| marker | Python | PyMuPDF + heuristics (no VLM) | [GitHub](https://github.com/VikParuchuri/marker) |
| nougat | Python | Facsimile model (no VLM) | [GitHub](https://github.com/facebookresearch/nougat) |

`edgequake-pdf2md` adopts the **PyZeroX architecture** in Rust, replacing:
- `poppler` → `pdfium-render` (higher quality, no external binary)
- `litellm` → `edgequake-llm` (native Rust multi-provider)
- `asyncio` → `tokio` + `futures`
