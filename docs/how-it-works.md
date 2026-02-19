# How It Works

`edgequake-pdf2md` converts PDF documents to clean Markdown using a six-stage pipeline powered by Vision Language Models (VLMs).

## Pipeline Overview

```
┌──────────────────────────────────────────────────────────────────────┐
│                     edgequake-pdf2md Pipeline                        │
├──────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────────────┐  │
│  │  INPUT    │──▶│ RENDER   │──▶│ ENCODE   │──▶│  VLM INFERENCE   │  │
│  │          │   │          │   │          │   │                  │  │
│  │ File/URL │   │ pdfium   │   │ base64   │   │  gpt-4.1-nano   │  │
│  │ resolve  │   │ rasterize│   │ PNG enc  │   │  claude-sonnet   │  │
│  └──────────┘   └──────────┘   └──────────┘   │  gemini-flash    │  │
│                                                └────────┬─────────┘  │
│                                                         │            │
│                                                         ▼            │
│                                   ┌─────────────────────────────┐    │
│                  ┌────────────┐   │    POST-PROCESS             │    │
│                  │  ASSEMBLE  │◀──│                             │    │
│                  │            │   │  • Strip fences             │    │
│                  │ Join pages │   │  • Fix tables               │    │
│                  │ Metadata   │   │  • Remove hallucinations    │    │
│                  │ Separators │   │  • Normalise whitespace     │    │
│                  └─────┬──────┘   └─────────────────────────────┘    │
│                        │                                             │
│                        ▼                                             │
│                  ┌────────────┐                                      │
│                  │  OUTPUT    │                                      │
│                  │  .md file  │                                      │
│                  │  or stdout │                                      │
│                  └────────────┘                                      │
└──────────────────────────────────────────────────────────────────────┘
```

## Stage Details

### Stage 1: Input Resolution

```
  input string
      │
      ├── starts with http:// or https://
      │   └── Download PDF to temp file
      │       (timeout: --download-timeout, default 120s)
      │
      └── local file path
          └── Validate exists, is readable, has %PDF magic
```

The input resolver accepts:
- **Local paths**: `/path/to/document.pdf`, `./paper.pdf`
- **HTTP/HTTPS URLs**: `https://arxiv.org/pdf/1706.03762`

Downloaded files are stored in a temp directory and cleaned up automatically.

### Stage 2: PDF Rasterisation (pdfium)

```
  PDF document
      │
      ├── Open with pdfium-render (libpdfium native binding)
      │
      ├── Read metadata (title, author, page count, etc.)
      │
      ├── Select pages (--pages flag)
      │   ├── all       → 0..N
      │   ├── 5         → [4]
      │   ├── 3-15      → 2..15
      │   └── 1,3,5,7   → [0,2,4,6]
      │
      └── Render each page to DynamicImage
          └── max_rendered_pixels × max_rendered_pixels
              (default: 2000px, set via --dpi)
```

Rasterisation runs in `spawn_blocking` since pdfium is CPU-bound. Each page becomes a `DynamicImage` (from the `image` crate).

### Stage 3: Base64 Encoding

Each rendered image is encoded to PNG format, then base64-encoded for inclusion in the VLM API request. The `ImageData` struct from `edgequake-llm` wraps the base64 string and MIME type.

### Stage 4: VLM Inference

```
  For each page image:
      │
      ├── Build system prompt (7 rules for faithful conversion)
      │
      ├── Optional: maintain_format context (previous page markdown)
      │
      ├── Send to VLM with image attachment
      │   └── Retry up to --max-retries times (exponential backoff)
      │
      └── Receive markdown text for that page
```

**Concurrency modes:**
- **Parallel** (default): Up to `--concurrency` pages processed simultaneously. Pages may complete out of order.
- **Sequential** (`--maintain-format`): Pages processed one at a time. Previous page's markdown is passed as context for format continuity.

**The system prompt** instructs the VLM to:
1. Preserve ALL text content accurately
2. Use proper heading hierarchy (# to ####)
3. Convert tables to GFM pipe format
4. Wrap code in fenced blocks
5. Render math as LaTeX ($inline$, $$display$$)
6. Ignore page numbers, headers/footers, decorative elements
7. Output only markdown (no fences, no commentary)

### Stage 5: Post-Processing

A 10-rule pipeline cleans the VLM output:

| Rule | Description |
|------|-------------|
| 1 | Strip markdown code fences (` ```markdown ... ```) |
| 2 | Normalise line endings (CRLF → LF) |
| 3 | Trim trailing whitespace |
| 4 | Collapse 3+ blank lines to 2 |
| 5 | Normalise heading spacing (blank line before `#`) |
| 6 | Fix broken table rows (re-join split pipes) |
| 7 | Remove mid-table separator rows |
| 8 | Remove hallucinated image references (`![...]()`) |
| 9 | Remove invisible Unicode characters (zero-width spaces, etc.) |
| 10 | Ensure final newline |

### Stage 6: Assembly

```
  ┌──────────────────────────────────────────────┐
  │  Optional: YAML front-matter (--metadata)    │
  │  ---                                         │
  │  title: "Paper Title"                        │
  │  author: "Author Name"                       │
  │  pages: 15                                   │
  │  ---                                         │
  │                                              │
  │  Page 1 markdown                             │
  │                                              │
  │  [separator: none | --- | <!-- page N -->]   │
  │                                              │
  │  Page 2 markdown                             │
  │  ...                                         │
  └──────────────────────────────────────────────┘
```

Pages are sorted by page number and joined with the configured separator (`--separator`).

## Fidelity Tiers

| Tier | What It Produces | Use Case |
|------|-----------------|----------|
| `tier1` | Text, headings, lists only | Fast extraction, text search |
| `tier2` | + GFM tables, footnotes | General documents (default) |
| `tier3` | + LaTeX math, HTML tables, image captions | Academic papers, textbooks |

Higher tiers use more detailed system prompts but produce richer output.

## Data Flow Summary

```
PDF → pdfium → DynamicImage[] → base64[] → VLM API → raw md[] → postprocess → clean md[] → joined .md
     ~~~~~~~~                              ~~~~~~~~~                                        ~~~~~~~~~~
     CPU-bound                             I/O-bound                                        CPU-bound
     (spawn_blocking)                      (concurrent async)                               (sync)
```

## Error Handling

- **Fatal errors** (`Pdf2MdError`): file not found, corrupt PDF, no provider → conversion aborts
- **Page errors** (`PageError`): single page render/LLM failure → skipped, other pages continue
- **All pages failed**: if zero pages succeed, returns `Pdf2MdError::AllPagesFailed`
- **Stats tracking**: `ConversionStats` reports processed/failed/skipped counts, token usage, timing
