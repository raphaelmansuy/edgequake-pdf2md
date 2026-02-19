# 02 — Core Algorithm & Pipeline

> **See also**: [Index](./00-index.md) · [Overview](./01-overview.md) · [PDF Internals](./03-pdf-internals.md) · [API Design](./06-api-design.md)

---

## 1. High-Level Pipeline

```
INPUT (path or URL)
       |
       v
+──────────────────────────────────────────────────────────────────+
│  Stage 0: Input Resolution                                       │
│  - Detect: local file path vs. HTTP/HTTPS URL                    │
│  - If URL: download to temp dir via reqwest (streaming)          │
│  - Validate: is it a PDF? (magic bytes %PDF-)                    │
│  - Apply: page selection filter (all | range | set)              │
+──────────────────────────────────────────────────────────────────+
       |
       v
+──────────────────────────────────────────────────────────────────+
│  Stage 1: PDF Loading (pdfium-render)                            │
│  - Load document handle (with optional password)                 │
│  - Extract metadata: page_count, title, author, page_sizes       │
│  - Emit document-level metadata into output struct               │
+──────────────────────────────────────────────────────────────────+
       |
       v  (per-page, via async stream)
+──────────────────────────────────────────────────────────────────+
│  Stage 2: Page Rasterisation (pdfium-render)                     │
│  - For each page (bounded by concurrency semaphore):             │
│    - Determine render target size: max(dpi*width, max_px)        │
│    - Render to RGBA bitmap via PdfRenderConfig                   │
│    - Convert: RGBA bitmap → PNG bytes via `image` crate          │
│    - Encode: PNG bytes → base64 String                           │
│    - Emit: (page_index, base64_png, page_metadata)               │
+──────────────────────────────────────────────────────────────────+
       |
       v  (via bounded tokio buffer_unordered)
+──────────────────────────────────────────────────────────────────+
│  Stage 3: Vision LLM Inference (edgequake-llm)                   │
│  - Build ChatMessage::user_with_images(prompt, [ImageData])      │
│  - Prepend system prompt (format-preservation instructions)       │
│  - If maintain_format=true: prepend prior page context as        │
│    assistant message to enable format continuity                  │
│  - Call provider.chat(&messages, options).await                  │
│  - Extract: LLMResponse.content → raw_markdown                   │
│  - Record: input_tokens, output_tokens, latency_ms               │
│  - On failure: retry with exponential backoff (max 3 attempts)   │
+──────────────────────────────────────────────────────────────────+
       |
       v
+──────────────────────────────────────────────────────────────────+
│  Stage 4: Markdown Post-Processing                               │
│  - Strip: ```markdown fences (some models wrap output)           │
│  - Normalise: trailing whitespace, CRLF → LF                     │
│  - Validate: well-formed Markdown (optional, via pulldown-cmark) │
│  - Emit: PageResult { page_num, markdown, tokens, latency }      │
+──────────────────────────────────────────────────────────────────+
       |
       v
+──────────────────────────────────────────────────────────────────+
│  Stage 5: Assembly                                               │
│  - Sort: PageResults by page_num (parallelism may reorder)       │
│  - Stitch: join with configurable page separator                 │
│  - Insert: page-break markers (optional)                         │
│  - Compute: aggregate token stats, total duration                │
│  - Emit: ConversionOutput { markdown, pages, stats }             │
+──────────────────────────────────────────────────────────────────+
       |
       v
+──────────────────────────────────────────────────────────────────+
│  Stage 6: Output                                                 │
│  - File path: write UTF-8 file (atomic via temp+rename)          │
│  - Stdout: write to tokio::io::stdout()                          │
│  - In-memory: return owned String                                │
│  - Streaming: yield PageResult via channel as pages complete     │
+──────────────────────────────────────────────────────────────────+
```

---

## 2. Concurrency Model

The key throughput lever is running multiple VLM API calls in parallel, since each call takes 1–10 seconds and the PDF rasterisation is fast by comparison.

```
tokio runtime (multi-thread)
│
├── Main task
│    │
│    ├── Rasteriser pool  ◄── bounded by Semaphore(raster_threads)
│    │    (CPU-bound: spawn_blocking)
│    │    Page 1 ──┐
│    │    Page 2 ──┤
│    │    Page 3 ──┤──► bitmap channel (bounded cap=concurrency)
│    │    ...     ──┘
│    │
│    └── LLM call pool   ◄── bounded by Semaphore(concurrency)
│         (I/O-bound: async)
│         Page 1 ──► VLM call ──► markdown
│         Page 2 ──► VLM call ──► markdown
│         ...
│         Page N ──► VLM call ──► markdown
│
└── Output collector (sort + assemble)
```

### Concurrency parameters

| Parameter | Default | Description |
|---|---|---|
| `concurrency` | 10 | Max simultaneous VLM API calls |
| `raster_threads` | `num_cpus::get()` | Max simultaneous rasterisation threads |
| `download_timeout_secs` | 120 | HTTP download timeout |
| `api_timeout_secs` | 60 | Per-VLM-call timeout |
| `retry_max` | 3 | Max retries on VLM failure |
| `retry_backoff_ms` | 500 | Initial retry delay (exponential) |

### Back-pressure mechanism

```
Rasterise produces faster than VLM consumes:
  → bitmap_channel fills up
  → rasteriser awaits send (natural back-pressure)
  → memory bounded to: concurrency * avg_bitmap_size

For 10 concurrent pages at 2000px wide (PNG ~800KB each):
  Peak bitmap memory ≈ 10 × 800KB = 8MB  (very manageable)
```

---

## 3. Pipeline Stages — Detail

### 3.1 Stage 0: Input Resolution

```
input: &str
  │
  ├── starts_with("http://") or "https://"?
  │    YES ──► reqwest::get(url)
  │              ├── stream body to tempfile
  │              ├── infer filename from Content-Disposition or URL path
  │              └── verify PDF magic bytes %PDF-
  │
  └── NO ──► path::canonicalize(input)
               ├── check file exists
               ├── check read permissions
               └── verify PDF magic bytes %PDF-

Page selection:
  None        ──► all pages [0..page_count]
  Single(n)   ──► [n-1] (1-indexed to 0-indexed)
  Range(a..=b)──► [a-1..b]
  Set(vec)    ──► sorted deduped vec, each 1-indexed
```

**Roadblocks**:
- Redirects (HTTP 301/302) — reqwest follows by default ✓
- URLs with query params — use Content-Disposition for filename
- Gzipped responses — reqwest decompresses by default ✓
- Large files — stream download, never load fully into memory

### 3.2 Stage 1: PDF Loading

```rust
// Pseudocode
let pdfium = Pdfium::default();
let doc = pdfium.load_pdf_from_file(&path, password)?;
let page_count = doc.pages().len();
let metadata = extract_metadata(&doc);
```

**Roadblocks**:
- Password-protected PDFs — `load_pdf_from_file(path, Some(password))`
- Linearised (fast web) PDFs — transparent to pdfium ✓
- Corrupt PDFs — pdfium returns `PdfiumError`; map to `Pdf2MdError::CorruptPdf`
- PDFs > 2GB — rare; pdfium handles via memory-mapped IO

### 3.3 Stage 2: Page Rasterisation

```
PdfPage
  │
  ├── page.width() in points (1 pt = 1/72 inch)
  ├── page.height() in points
  │
  ▼
target_width_px  = ceil(width_pts / 72.0 * dpi)   // e.g. 595pt/72*150 = 1240px
target_height_px = ceil(height_pts / 72.0 * dpi)  // e.g. 842pt/72*150 = 1754px
  │
  ▼
PdfRenderConfig::new()
    .set_target_width(target_width_px)
    .set_maximum_height(target_height_px)
    .rotate_if_landscape(PdfPageRenderRotation::Degrees90, true)
  │
  ▼
page.render_with_config(&config)?  ──► PdfBitmap (BGRA or RGBA)
  │
  ▼
.as_image()  ──► image::DynamicImage
  │
  ▼
.into_rgb8() ──► image::RgbImage           // drop alpha for JPEG
 OR
keep RGBA   ──► encode as PNG (lossless)   // prefer for quality
  │
  ▼
PNG encode ──► Vec<u8>
  │
  ▼
base64::engine::general_purpose::STANDARD.encode(bytes) ──► String
```

**DPI selection guidance**:

```
DPI  | width_px (A4)  | file_size (PNG) | LLM quality | Cost
-----|----------------|-----------------|-------------|-----
 72  |   595px        |   ~80KB         | poor        | $
150  |  1240px        |  ~300KB         | good        | $$
200  |  1654px        |  ~500KB         | very good   | $$$
300  |  2480px        |  ~1.2MB         | excellent   | $$$$

Default: 150 DPI (optimal quality/cost tradeoff for most docs)
Max recommended: 300 DPI (for scanned docs, small fonts)
```

### 3.4 Stage 3: Vision LLM Inference

```
(page_index, base64_png, prior_page_markdown?)
  │
  ▼
System message:
  "Convert the provided PDF page image to Markdown format.
   Preserve all text, tables, code blocks, and mathematical
   formulas. Use proper Markdown syntax. Do not add
   commentary. Output only the Markdown content."

  [optional, if maintain_format=true]:
  append prior page markdown as assistant context message
  │
  ▼
User message: ChatMessage::user_with_images(
    content: prompt_text,
    images: vec![ImageData::new(base64_png, "image/png")
                    .with_detail("high")]
)
  │
  ▼
provider.chat(&[system_msg, user_msg], Some(&options)).await
  │
  ├── Ok(response)  ──► response.content  (raw markdown string)
  │                      response.usage.input_tokens
  │                      response.usage.output_tokens
  │
  └── Err(e)  ──► match e {
                    LlmError::RateLimit(_) ──► wait + retry
                    LlmError::Timeout(_)   ──► retry up to max_retries
                    LlmError::ApiError(_)  ──► retry if 5xx, fail if 4xx
                    _                      ──► fail immediately
                  }
```

**The system prompt** is the critical quality lever. See [04-markdown-spec.md §System Prompt](./04-markdown-spec.md#system-prompt) for the full prompt specification.

### 3.5 Stage 4: Markdown Post-Processing

```
raw_markdown: String
  │
  ├── Strip: leading/trailing ```markdown ... ``` fences
  │     regex: r"^```(?:markdown)?\n?([\s\S]*?)\n?```$"
  │
  ├── Normalise: \r\n → \n
  │
  ├── Normalise: trailing spaces per line
  │
  ├── Collapse: >3 consecutive blank lines → 2 blank lines
  │
  └── Result: clean_markdown: String
```

**Why strip fences?** GPT-4o, Claude, and Gemini occasionally wrap output in
markdown code fences despite the system prompt requesting plain markdown.
The stripping is conservative: only the outermost fence is removed if the
entire response is wrapped.

### 3.6 Stage 5: Assembly

```
Vec<PageResult { page_num, markdown, ... }>
  │
  ├── Sort by page_num (ascending)
  │
  ├── Inject page separators?
  │    Some(sep) ──► join with "\n\n---\n\n" (configurable)
  │    None      ──► join with "\n\n"
  │
  ├── Prepend document metadata block? (opt-in)
  │    "---\ntitle: {title}\nauthor: {author}\n---\n\n"
  │
  └── ConversionOutput {
          markdown: String,
          pages: Vec<PageResult>,
          stats: ConversionStats {
              total_pages: usize,
              processed_pages: usize,
              failed_pages: usize,
              input_tokens: u64,
              output_tokens: u64,
              duration_ms: u64,
          }
      }
```

---

## 4. Maintain-Format Mode

When `maintain_format = true`, each page is processed **sequentially** (not in parallel), and the previous page's Markdown is injected as context for the next LLM call:

```
Page 1 ──► VLM ──► md_1 ──┐
                            ├──► VLM ──► md_2 ──┐
                            │                    ├──► VLM ──► md_3 ...
                            │ [context: md_1]    │ [context: md_2]
                            └────────────────────┘

Message structure per page N (N > 1):
  [system_msg]
  [user_msg_N-1 (prev image + prompt)]
  [assistant_msg (md_N-1)]
  [user_msg_N (curr image + prompt)]
```

**When to use**: Documents with running headers, continuous numbering, cross-references, or narrative flow that spans page boundaries. The tradeoff is 10× slower throughput (sequential vs. parallel).

---

## 5. Streaming Mode

For real-time UX (e.g. feeding to a display or downstream pipeline):

```
convert_stream(input, config) ──► impl Stream<Item = Result<PageResult>>

Consumer:
  while let Some(page) = stream.next().await {
      match page {
          Ok(p)  => println!("Page {}: {} chars", p.page_num, p.markdown.len()),
          Err(e) => eprintln!("Page error: {e}"),
      }
  }
```

Pages are emitted as they complete (out of order during parallel mode). The consumer must sort if order matters.

---

## 6. LLM Integration

`edgequake-llm` API used:

```rust
use edgequake_llm::{
    ChatMessage,
    ImageData,
    CompletionOptions,
    ProviderFactory,
    ProviderType,
    LLMProvider,
};

// Build provider from environment variables
let provider = ProviderFactory::from_env()?;

// Build multimodal message
let image = ImageData::new(base64_png_string, "image/png")
    .with_detail("high");

let messages = vec![
    ChatMessage::system(SYSTEM_PROMPT),
    ChatMessage::user_with_images(user_prompt, vec![image]),
];

let options = CompletionOptions {
    temperature: Some(0.1),  // low temp for deterministic OCR-like output
    max_tokens: Some(4096),
    ..Default::default()
};

let response = provider.chat(&messages, Some(&options)).await?;
let markdown = response.content;
```

**Provider selection via environment variables** (delegated to `edgequake-llm` factory):

```
EDGEQUAKE_PROVIDER=openai      OPENAI_API_KEY=sk-...
EDGEQUAKE_PROVIDER=anthropic   ANTHROPIC_API_KEY=sk-ant-...
EDGEQUAKE_PROVIDER=gemini      GEMINI_API_KEY=...
EDGEQUAKE_PROVIDER=ollama      OLLAMA_BASE_URL=http://localhost:11434
EDGEQUAKE_PROVIDER=azure       AZURE_OPENAI_API_KEY=... AZURE_OPENAI_ENDPOINT=...
```

Alternatively, provider can be specified programmatically via `ConversionConfig::provider`.

---

## 7. Data Flow — Type-Level View

```
Input: &str (path or URL)
  │
  ▼ Stage 0
TempFile (path: PathBuf)
  │
  ▼ Stage 1
PdfDocumentHandle<'_> + DocumentMetadata
  │
  ▼ Stage 2 (per page)
PageBitmap {
    page_num: usize,      // 1-indexed
    width_px: u32,
    height_px: u32,
    base64_png: String,   // ~100KB–2MB encoded
    render_ms: u64,
}
  │
  ▼ Stage 3 (per page, async)
RawPageResult {
    page_num: usize,
    raw_markdown: String,
    input_tokens: u32,
    output_tokens: u32,
    llm_latency_ms: u64,
    attempt: u8,
}
  │
  ▼ Stage 4 (per page)
PageResult {
    page_num: usize,
    markdown: String,          // cleaned
    input_tokens: u32,
    output_tokens: u32,
    total_latency_ms: u64,
}
  │
  ▼ Stage 5
ConversionOutput {
    markdown: String,          // full document
    pages: Vec<PageResult>,
    metadata: DocumentMetadata,
    stats: ConversionStats,
}
```

---

## 8. Edge Cases Handled at the Algorithm Level

| Edge Case | Handling |
|---|---|
| 0-page PDF | Return Ok(empty output) with warning |
| Single-page PDF | Works identically to multi-page |
| Landscape pages | `rotate_if_landscape` in render config |
| Mixed portrait/landscape | Per-page rotation detection |
| Very large pages (A0, posters) | `max_width_px` cap prevents OOM |
| Encrypted/password PDF | Pass `password` param to pdfium |
| Pages with no text (blank) | VLM returns empty/minimal markdown; accepted |
| VLM hallucination | Only stripping fences and whitespace; no semantic filtering |
| VLM max_tokens exceeded | Truncated response is kept; warning emitted |
| API rate limit | Exponential backoff with jitter |
| Network timeout (download) | Configurable timeout; clear error message |
| Pages selected out of range | `Pdf2MdError::PageOutOfRange` before processing |
| Duplicate pages in selection | Deduplicated silently |

---

## 9. Performance Estimates

```
Benchmark setup:
  Document: 50-page academic paper (A4, mixed text+figures+tables)
  Provider: OpenAI gpt-4o
  DPI: 150
  Concurrency: 10
  Machine: MacBook Pro M3 (8 cores)

  Rasterisation:     50 pages × ~50ms  = 2.5s  (parallel, 8 threads)
  LLM calls:         50 pages × ~3s    = 15s   (10 concurrency → ceil(50/10)×3 = 15s)
  Post-processing:   50 pages × ~1ms   = 0.05s
  Assembly:                             ~1ms
  ─────────────────────────────────────────────
  Total wall time:   ~18 seconds
  Token usage:       ~50 pages × 1000 input + 500 output ≈ 75K tokens
  Cost (gpt-4o):     ~$0.60
```

For details on token estimation per page, see [03-pdf-internals.md §Token Budget](./03-pdf-internals.md#token-budget).
