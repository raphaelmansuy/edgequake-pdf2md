# 06 — Library API Design

> **See also**: [Index](./00-index.md) · [Algorithm](./02-algorithm.md) · [CLI Design](./07-cli-design.md) · [Error Handling](./08-error-handling.md)

---

## 1. Design Philosophy

The public API follows these principles:

1. **Minimal surface area** — expose the fewest types necessary to accomplish the task
2. **Builder pattern** — `ConversionConfig::builder()` prevents API-breaking additions
3. **Async-first** — all I/O-bound operations return `Future`s
4. **Sync convenience** — `convert_sync()` wrapper for non-async callers
5. **Streaming optional** — both eager (return full doc) and lazy (stream pages) APIs
6. **Provider-agnostic** — `dyn LLMProvider` or `ProviderFactory::from_env()` sourced provider
7. **Fail gracefully** — page-level errors never abort the document conversion

---

## 2. Module Structure

```
edgequake_pdf2md
├── lib.rs                    (re-exports, crate docs)
├── config.rs                 (ConversionConfig, builder)
├── error.rs                  (Pdf2MdError, PageError)
├── output.rs                 (ConversionOutput, PageResult, ConversionStats)
├── convert.rs                (primary async API functions)
├── stream.rs                 (streaming API)
├── pipeline/
│   ├── input.rs              (input resolution, URL download)
│   ├── render.rs             (pdfium page rasterisation)
│   ├── encode.rs             (bitmap → base64)
│   ├── llm.rs                (VLM API calls via edgequake-llm)
│   └── postprocess.rs        (markdown cleaning)
└── bin/
    └── pdf2md.rs             (CLI entry point)
```

---

## 3. Core Types

### 3.1 ConversionConfig

```rust
/// Configuration for a PDF-to-Markdown conversion.
///
/// Built via [`ConversionConfig::builder()`] or using struct update syntax
/// from [`ConversionConfig::default()`].
///
/// # Example
/// ```rust
/// use edgequake_pdf2md::ConversionConfig;
///
/// let config = ConversionConfig::builder()
///     .dpi(150)
///     .concurrency(10)
///     .model("gpt-4o")
///     .build()
///     .unwrap();
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversionConfig {
    /// Rendering DPI (dots per inch).
    /// Higher DPI = better quality but more tokens and cost.
    /// Range: 72–400. Default: 150.
    pub dpi: u32,

    /// Maximum rendered image dimension (width or height) in pixels.
    /// Prevents OOM on very large pages. Default: 2000.
    pub max_rendered_pixels: u32,

    /// Number of concurrent VLM API calls. Default: 10.
    pub concurrency: usize,

    /// Maximum number of rasterisation worker threads.
    /// Default: num_cpus::get().
    pub raster_threads: usize,

    /// LLM model identifier, e.g. "gpt-4o", "claude-3-5-sonnet-20241022".
    /// If None, uses EDGEQUAKE_MODEL env var or provider default.
    pub model: Option<String>,

    /// LLM provider. If None, uses ProviderFactory::from_env().
    pub provider: Option<Arc<dyn LLMProvider>>,

    /// Temperature for LLM completion. Lower = more deterministic.
    /// Default: 0.1.
    pub temperature: f32,

    /// Maximum tokens for LLM output per page. Default: 4096.
    pub max_tokens: u32,

    /// Maximum retry attempts on VLM API failure. Default: 3.
    pub max_retries: u32,

    /// Initial retry delay in milliseconds (exponential backoff). Default: 500.
    pub retry_backoff_ms: u64,

    /// PDF user password for encrypted documents. Default: None.
    pub password: Option<String>,

    /// Custom system prompt. If None, uses built-in default prompt.
    pub system_prompt: Option<String>,

    /// Whether to process pages sequentially, passing previous page
    /// as context to the next VLM call. Improves format continuity
    /// but disables parallelism. Default: false.
    pub maintain_format: bool,

    /// Fidelity tier for output quality. Default: Tier2.
    pub fidelity: FidelityTier,

    /// Page selection. Default: All pages.
    pub pages: PageSelection,

    /// Page separator in assembled output. Default: None.
    pub page_separator: PageSeparator,

    /// Include YAML front-matter with document metadata. Default: false.
    pub include_metadata: bool,

    /// Extract embedded images as separate files. Default: false.
    pub extract_images: bool,

    /// Download timeout for URL inputs in seconds. Default: 120.
    pub download_timeout_secs: u64,

    /// Per-VLM-call timeout in seconds. Default: 60.
    pub api_timeout_secs: u64,
}

impl ConversionConfig {
    pub fn builder() -> ConversionConfigBuilder { ... }
}

impl Default for ConversionConfig {
    fn default() -> Self { ... }
}
```

### 3.2 FidelityTier

```rust
/// Quality tier for the Markdown output.
///
/// Higher tiers produce better output but require more capable models
/// and consume more tokens. See [`04-markdown-spec.md`] for details.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FidelityTier {
    /// Basic: text, headings, lists. Tables may be linearised.
    Tier1,
    /// Structural: text, headings, lists, GFM tables, footnotes. (default)
    #[default]
    Tier2,
    /// High-fidelity: Tier2 + LaTeX math, HTML table fallback, image captions.
    Tier3,
}
```

### 3.3 PageSelection

```rust
/// Specifies which pages of the PDF to convert.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum PageSelection {
    /// Convert all pages (default).
    #[default]
    All,
    /// Convert a single page (1-indexed).
    Single(usize),
    /// Convert a contiguous range of pages (1-indexed, inclusive).
    Range(std::ops::RangeInclusive<usize>),
    /// Convert specific pages (1-indexed, any order; deduplicated).
    Set(Vec<usize>),
}
```

### 3.4 PageSeparator

```rust
/// How to separate pages in the assembled Markdown output.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum PageSeparator {
    /// No separator; pages joined with "\n\n". (default)
    #[default]
    None,
    /// Horizontal rule: "\n\n---\n\n"
    HorizontalRule,
    /// HTML comment with page number: "<!-- page N -->"
    Comment,
    /// Custom string inserted between pages.
    Custom(String),
}
```

### 3.5 ConversionOutput

```rust
/// The result of a successful conversion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversionOutput {
    /// The complete Markdown document.
    pub markdown: String,

    /// Per-page results in page order.
    pub pages: Vec<PageResult>,

    /// Document metadata extracted from PDF.
    pub metadata: DocumentMetadata,

    /// Aggregate statistics for the conversion.
    pub stats: ConversionStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageResult {
    /// Page number (1-indexed).
    pub page_num: usize,
    /// Markdown content for this page.
    pub markdown: String,
    /// LLM input tokens consumed.
    pub input_tokens: u32,
    /// LLM output tokens consumed.
    pub output_tokens: u32,
    /// Total wall-clock time for this page (render + LLM + post).
    pub duration_ms: u64,
    /// Number of retry attempts made (0 = succeeded first time).
    pub retries: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversionStats {
    pub total_pages: usize,
    pub processed_pages: usize,
    pub failed_pages: usize,
    pub skipped_pages: usize,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_duration_ms: u64,
    pub render_duration_ms: u64,
    pub llm_duration_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DocumentMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub creator: Option<String>,
    pub producer: Option<String>,
    pub creation_date: Option<String>,
    pub modification_date: Option<String>,
    pub page_count: usize,
    pub pdf_version: String,   // e.g. "1.7"
    pub is_encrypted: bool,
    pub is_linearised: bool,
}
```

---

## 4. Primary API Functions

### 4.1 Eager Conversion (full document in one call)

```rust
/// Convert a PDF file or URL to Markdown.
///
/// This is the primary entry point for the library.
///
/// # Arguments
/// * `input` - Local file path or HTTP/HTTPS URL to a PDF
/// * `config` - Conversion configuration
///
/// # Returns
/// `Ok(ConversionOutput)` on success, even if some pages failed
/// (check `output.stats.failed_pages`).
///
/// # Errors
/// Returns `Err(Pdf2MdError)` only for fatal errors:
/// - File not found / permission denied
/// - Not a valid PDF
/// - All pages failed and no output produced
///
/// # Example
/// ```rust,no_run
/// # use edgequake_pdf2md::{convert, ConversionConfig};
/// # #[tokio::main]
/// # async fn main() -> anyhow::Result<()> {
/// let config = ConversionConfig::default();
/// let output = convert("report.pdf", &config).await?;
/// println!("{}", output.markdown);
/// # Ok(())
/// # }
/// ```
pub async fn convert(
    input: impl AsRef<str>,
    config: &ConversionConfig,
) -> Result<ConversionOutput, Pdf2MdError> { ... }
```

### 4.2 Streaming Conversion (pages emitted as they complete)

```rust
/// Convert a PDF to Markdown, streaming pages as they are ready.
///
/// Pages are emitted in completion order (not necessarily page order)
/// when `maintain_format = false`. Sort by `page_num` if order matters.
///
/// # Example
/// ```rust,no_run
/// # use edgequake_pdf2md::{convert_stream, ConversionConfig};
/// # use tokio_stream::StreamExt;
/// # #[tokio::main]
/// # async fn main() -> anyhow::Result<()> {
/// let config = ConversionConfig::default();
/// let mut stream = convert_stream("report.pdf", &config).await?;
/// while let Some(page) = stream.next().await {
///     match page {
///         Ok(p) => eprintln!("Page {} done: {} chars", p.page_num, p.markdown.len()),
///         Err(e) => eprintln!("Page error: {e}"),
///     }
/// }
/// # Ok(())
/// # }
/// ```
pub async fn convert_stream(
    input: impl AsRef<str>,
    config: &ConversionConfig,
) -> Result<impl Stream<Item = Result<PageResult, PageError>>, Pdf2MdError> { ... }
```

### 4.3 Synchronous Convenience Wrapper

```rust
/// Synchronous wrapper around [`convert`].
///
/// Creates a temporary tokio runtime internally. For library users who
/// cannot use async code. Performance-equivalent to the async version.
///
/// # Example
/// ```rust,no_run
/// # use edgequake_pdf2md::{convert_sync, ConversionConfig};
/// let config = ConversionConfig::default();
/// let output = convert_sync("report.pdf", &config)?;
/// ```
pub fn convert_sync(
    input: impl AsRef<str>,
    config: &ConversionConfig,
) -> Result<ConversionOutput, Pdf2MdError> {
    tokio::runtime::Runtime::new()
        .expect("failed to create tokio runtime")
        .block_on(convert(input, config))
}
```

### 4.4 Conversion to File

```rust
/// Convert a PDF and write output directly to a file.
///
/// Uses atomic write (temp file + rename) to prevent partial files.
///
/// # Example
/// ```rust,no_run
/// # use edgequake_pdf2md::{convert_to_file, ConversionConfig};
/// # #[tokio::main]
/// # async fn main() -> anyhow::Result<()> {
/// let config = ConversionConfig::default();
/// let stats = convert_to_file("report.pdf", "output.md", &config).await?;
/// println!("Converted {} pages in {}ms", stats.total_pages, stats.total_duration_ms);
/// # Ok(())
/// # }
/// ```
pub async fn convert_to_file(
    input: impl AsRef<str>,
    output_path: impl AsRef<Path>,
    config: &ConversionConfig,
) -> Result<ConversionStats, Pdf2MdError> { ... }
```

### 4.5 PDF Metadata Extraction (no LLM)

```rust
/// Extract PDF metadata without converting content.
///
/// Does not require an LLM provider or API key.
///
/// # Example
/// ```rust,no_run
/// # use edgequake_pdf2md::{inspect, ConversionConfig};
/// # #[tokio::main]
/// # async fn main() -> anyhow::Result<()> {
/// let meta = inspect("report.pdf").await?;
/// println!("Title: {:?}, Pages: {}", meta.title, meta.page_count);
/// # Ok(())
/// # }
/// ```
pub async fn inspect(
    input: impl AsRef<str>,
) -> Result<DocumentMetadata, Pdf2MdError> { ... }
```

---

## 5. Builder Pattern

```rust
/// Builder for [`ConversionConfig`].
///
/// Obtainable via [`ConversionConfig::builder()`].
pub struct ConversionConfigBuilder {
    config: ConversionConfig,
}

impl ConversionConfigBuilder {
    pub fn dpi(mut self, dpi: u32) -> Self {
        self.config.dpi = dpi.clamp(72, 400);
        self
    }

    pub fn concurrency(mut self, n: usize) -> Self {
        self.config.concurrency = n.max(1);
        self
    }

    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.config.model = Some(model.into());
        self
    }

    pub fn provider(mut self, provider: Arc<dyn LLMProvider>) -> Self {
        self.config.provider = Some(provider);
        self
    }

    pub fn maintain_format(mut self, v: bool) -> Self {
        self.config.maintain_format = v;
        self
    }

    pub fn pages(mut self, selection: PageSelection) -> Self {
        self.config.pages = selection;
        self
    }

    pub fn password(mut self, pwd: impl Into<String>) -> Self {
        self.config.password = Some(pwd.into());
        self
    }

    pub fn fidelity(mut self, tier: FidelityTier) -> Self {
        self.config.fidelity = tier;
        self
    }

    pub fn system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.config.system_prompt = Some(prompt.into());
        self
    }

    pub fn page_separator(mut self, sep: PageSeparator) -> Self {
        self.config.page_separator = sep;
        self
    }

    pub fn include_metadata(mut self, v: bool) -> Self {
        self.config.include_metadata = v;
        self
    }

    pub fn max_tokens(mut self, n: u32) -> Self {
        self.config.max_tokens = n;
        self
    }

    pub fn temperature(mut self, t: f32) -> Self {
        self.config.temperature = t.clamp(0.0, 2.0);
        self
    }

    pub fn build(self) -> Result<ConversionConfig, ConfigError> {
        // Validate: e.g. if maintain_format + pages=All for large doc → warn
        Ok(self.config)
    }
}
```

---

## 6. Usage Examples

### Example 1 — Simplest possible call

```rust
use edgequake_pdf2md::convert;

// Uses all defaults: 150 DPI, concurrency 10, provider from env
let output = convert("document.pdf", &Default::default()).await?;
println!("{}", output.markdown);
```

### Example 2 — Custom provider (Anthropic Claude)

```rust
use edgequake_pdf2md::{convert, ConversionConfig};
use edgequake_llm::{AnthropicProvider, LLMProvider};
use std::sync::Arc;

let provider = AnthropicProvider::from_env()?;
let config = ConversionConfig::builder()
    .provider(Arc::new(provider))
    .model("claude-3-5-sonnet-20241022")
    .fidelity(FidelityTier::Tier3)  // LaTeX math enabled
    .dpi(200)
    .build()?;

let output = convert("math_paper.pdf", &config).await?;
```

### Example 3 — Convert specific pages, stream results

```rust
use edgequake_pdf2md::{convert_stream, ConversionConfig, PageSelection};
use tokio_stream::StreamExt;

let config = ConversionConfig::builder()
    .pages(PageSelection::Range(5..=15))
    .concurrency(5)
    .build()?;

let mut stream = convert_stream("large_report.pdf", &config).await?;

let mut results = vec![];
while let Some(r) = stream.next().await {
    let page = r?;
    eprintln!("Page {} ready", page.page_num);
    results.push(page);
}

results.sort_by_key(|p| p.page_num);
```

### Example 4 — Local LLM via Ollama

```rust
use edgequake_pdf2md::{convert, ConversionConfig};
use edgequake_llm::OllamaProvider;
use std::sync::Arc;

let provider = OllamaProvider::new("http://localhost:11434", "llava:34b");
let config = ConversionConfig::builder()
    .provider(Arc::new(provider))
    .dpi(150)
    .concurrency(2)  // local model is slower
    .build()?;

let output = convert("document.pdf", &config).await?;
```

### Example 5 — URL input, write to file

```rust
use edgequake_pdf2md::convert_to_file;

let stats = convert_to_file(
    "https://arxiv.org/pdf/2310.12345",
    "paper.md",
    &Default::default()
).await?;

println!("Done: {} pages, {}ms, {} tokens",
    stats.total_pages, stats.total_duration_ms,
    stats.total_input_tokens + stats.total_output_tokens);
```

---

## 7. Public API Summary

```
edgequake_pdf2md
├── fn convert(input, config) -> Future<ConversionOutput>
├── fn convert_stream(input, config) -> Future<Stream<PageResult>>
├── fn convert_to_file(input, out_path, config) -> Future<ConversionStats>
├── fn convert_sync(input, config) -> ConversionOutput
├── fn inspect(input) -> Future<DocumentMetadata>
│
├── struct ConversionConfig         (builder + Default)
├── struct ConversionConfigBuilder
├── enum   FidelityTier             (Tier1 | Tier2 | Tier3)
├── enum   PageSelection            (All | Single | Range | Set)
├── enum   PageSeparator            (None | HorizontalRule | Comment | Custom)
│
├── struct ConversionOutput         (markdown, pages, metadata, stats)
├── struct PageResult               (page_num, markdown, tokens, timing)
├── struct ConversionStats
├── struct DocumentMetadata
│
└── enum   Pdf2MdError              (see 08-error-handling.md)
```

All public types implement `Debug`, `Clone`, `Serialize`, `Deserialize`.
