//! Configuration types for PDF-to-Markdown conversion.
//!
//! All conversion behaviour is controlled through [`ConversionConfig`], built
//! via its [`ConversionConfigBuilder`]. Keeping every knob in one struct makes
//! it trivial to share configs across threads, serialise them for logging, and
//! diff two runs to understand why their outputs differ.
//!
//! # Design choice: builder over constructor
//! A twenty-field constructor is unreadable and breaks on every new field.
//! The builder pattern lets callers set only what they care about and rely on
//! well-documented defaults for the rest.

use crate::error::Pdf2MdError;
use edgequake_llm::LLMProvider;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;

/// Configuration for a PDF-to-Markdown conversion.
///
/// Built via [`ConversionConfig::builder()`] or using
/// [`ConversionConfig::default()`].
///
/// # Example
/// ```rust
/// use edgequake_pdf2md::ConversionConfig;
///
/// let config = ConversionConfig::builder()
///     .dpi(150)
///     .concurrency(10)
///     .model("gpt-4.1-nano")
///     .build()
///     .unwrap();
/// ```
#[derive(Clone)]
pub struct ConversionConfig {
    /// Rendering DPI used when rasterising each PDF page. Range: 72–400. Default: 150.
    ///
    /// 150 DPI is the sweet spot: text is sharp enough for a VLM to read reliably,
    /// while image file sizes stay well below typical API upload limits (~20 MB).
    /// Increase to 200–300 for small-font documents; decrease to 96 for very large
    /// pages where file size matters more than pixel density.
    pub dpi: u32,

    /// Maximum rendered image dimension (width or height) in pixels. Default: 2000.
    ///
    /// A safety cap independent of DPI. A 200-DPI render of an A0 poster could
    /// produce a 13 000 × 18 000 px image and exhaust memory. This field caps
    /// either dimension, scaling the other proportionally, so pdfium never
    /// allocates more than roughly `max_rendered_pixels²` bytes of pixels.
    pub max_rendered_pixels: u32,

    /// Number of concurrent VLM API calls. Default: 10.
    ///
    /// VLM APIs are network-bound, not CPU-bound. Issuing 10 calls at once
    /// typically cuts wall-clock time by 8–9× compared to sequential conversion.
    /// If you hit rate-limit errors (`429`), lower this; if the API is fast and
    /// your network is wide, you can raise it safely.
    pub concurrency: usize,

    /// LLM model identifier, e.g. "gpt-4o", "claude-sonnet-4-20250514".
    /// If None, uses provider default.
    pub model: Option<String>,

    /// LLM provider name (e.g. "openai", "anthropic", "ollama").
    /// If None along with `provider`, uses `ProviderFactory::from_env()`.
    pub provider_name: Option<String>,

    /// Pre-constructed LLM provider. Takes precedence over `provider_name`.
    pub provider: Option<Arc<dyn LLMProvider>>,

    /// Sampling temperature for the LLM completion. Default: 0.1.
    ///
    /// Low temperature (close to 0) makes the model deterministic and faithful
    /// to what it sees on the page — exactly what you want for transcription.
    /// Higher values introduce creativity that worsens OCR accuracy.
    pub temperature: f32,

    /// Maximum tokens the LLM may generate per page. Default: 4096.
    ///
    /// Dense pages (tables, code listings) can exceed 2 000 output tokens.
    /// Setting this too low silently truncates the Markdown mid-sentence.
    /// 4 096 covers the 99th percentile of academic-paper pages while keeping
    /// per-page cost predictable.
    pub max_tokens: usize,

    /// Maximum retry attempts on a transient VLM API failure. Default: 3.
    ///
    /// Most 5xx and timeout errors are transient (overloaded backend, network
    /// blip). Retrying 3 times catches the vast majority without blocking the
    /// pipeline for seconds. Permanent errors (bad API key, 400) are not
    /// retried — they surface as [`crate::error::PageError`] immediately.
    pub max_retries: u32,

    /// Initial retry delay in milliseconds (exponential backoff). Default: 500.
    ///
    /// Doubles after each attempt: 500 ms → 1 s → 2 s. Exponential backoff
    /// avoids the thundering-herd problem where N concurrent workers retry
    /// simultaneously and immediately overwhelm a recovering API endpoint.
    pub retry_backoff_ms: u64,

    /// PDF user password for encrypted documents.
    pub password: Option<String>,

    /// Custom system prompt. If None, uses built-in default.
    pub system_prompt: Option<String>,

    /// Sequential mode: pass the previous page's Markdown as context to the VLM. Default: false.
    ///
    /// **Why it helps:** VLMs do not inherently know that page 3 continues the
    /// numbered list from page 2. Passing the prior page's output as a context
    /// message lets the model continue lists, match heading levels, and avoid
    /// re-introducing section titles that already appeared.
    ///
    /// **The trade-off:** Sequential mode forces pages to be processed one at a
    /// time (concurrency is effectively 1). For a 100-page document this can
    /// take 5–10× longer than parallel mode. Enable it for books and reports
    /// where formatting continuity matters; leave it off for collections of
    /// independent pages (slide decks, scanned invoices).
    pub maintain_format: bool,

    /// Fidelity tier controlling prompt complexity and output richness. Default: [`FidelityTier::Tier2`].
    ///
    /// Higher tiers instruct the VLM to handle more complex constructs (LaTeX,
    /// HTML table fallback, image captions), which costs more tokens and may
    /// slightly slow responses. Tier2 is the right default for most documents.
    pub fidelity: FidelityTier,

    /// Page selection. Default: All pages.
    pub pages: PageSelection,

    /// Page separator in assembled output. Default: None.
    pub page_separator: PageSeparator,

    /// Include YAML front-matter with document metadata. Default: false.
    pub include_metadata: bool,

    /// Download timeout for URL inputs in seconds. Default: 120.
    pub download_timeout_secs: u64,

    /// Per-VLM-call timeout in seconds. Default: 60.
    pub api_timeout_secs: u64,
}

impl Default for ConversionConfig {
    fn default() -> Self {
        Self {
            dpi: 150,
            max_rendered_pixels: 2000,
            concurrency: 10,
            model: None,
            provider_name: None,
            provider: None,
            temperature: 0.1,
            max_tokens: 4096,
            max_retries: 3,
            retry_backoff_ms: 500,
            password: None,
            system_prompt: None,
            maintain_format: false,
            fidelity: FidelityTier::default(),
            pages: PageSelection::default(),
            page_separator: PageSeparator::default(),
            include_metadata: false,
            download_timeout_secs: 120,
            api_timeout_secs: 60,
        }
    }
}

impl fmt::Debug for ConversionConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConversionConfig")
            .field("dpi", &self.dpi)
            .field("max_rendered_pixels", &self.max_rendered_pixels)
            .field("concurrency", &self.concurrency)
            .field("model", &self.model)
            .field("provider_name", &self.provider_name)
            .field("provider", &self.provider.as_ref().map(|_| "<dyn LLMProvider>"))
            .field("temperature", &self.temperature)
            .field("max_tokens", &self.max_tokens)
            .field("max_retries", &self.max_retries)
            .field("maintain_format", &self.maintain_format)
            .field("fidelity", &self.fidelity)
            .field("pages", &self.pages)
            .field("page_separator", &self.page_separator)
            .finish()
    }
}

impl ConversionConfig {
    /// Create a new builder for `ConversionConfig`.
    pub fn builder() -> ConversionConfigBuilder {
        ConversionConfigBuilder {
            config: Self::default(),
        }
    }
}

/// Builder for [`ConversionConfig`].
#[derive(Debug)]
pub struct ConversionConfigBuilder {
    config: ConversionConfig,
}

impl ConversionConfigBuilder {
    pub fn dpi(mut self, dpi: u32) -> Self {
        self.config.dpi = dpi.clamp(72, 400);
        self
    }

    pub fn max_rendered_pixels(mut self, px: u32) -> Self {
        self.config.max_rendered_pixels = px.max(100);
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

    pub fn provider_name(mut self, name: impl Into<String>) -> Self {
        self.config.provider_name = Some(name.into());
        self
    }

    pub fn provider(mut self, provider: Arc<dyn LLMProvider>) -> Self {
        self.config.provider = Some(provider);
        self
    }

    pub fn temperature(mut self, t: f32) -> Self {
        self.config.temperature = t.clamp(0.0, 2.0);
        self
    }

    pub fn max_tokens(mut self, n: usize) -> Self {
        self.config.max_tokens = n;
        self
    }

    pub fn max_retries(mut self, n: u32) -> Self {
        self.config.max_retries = n;
        self
    }

    pub fn retry_backoff_ms(mut self, ms: u64) -> Self {
        self.config.retry_backoff_ms = ms;
        self
    }

    pub fn password(mut self, pwd: impl Into<String>) -> Self {
        self.config.password = Some(pwd.into());
        self
    }

    pub fn system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.config.system_prompt = Some(prompt.into());
        self
    }

    pub fn maintain_format(mut self, v: bool) -> Self {
        self.config.maintain_format = v;
        self
    }

    pub fn fidelity(mut self, tier: FidelityTier) -> Self {
        self.config.fidelity = tier;
        self
    }

    pub fn pages(mut self, selection: PageSelection) -> Self {
        self.config.pages = selection;
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

    pub fn download_timeout_secs(mut self, secs: u64) -> Self {
        self.config.download_timeout_secs = secs;
        self
    }

    pub fn api_timeout_secs(mut self, secs: u64) -> Self {
        self.config.api_timeout_secs = secs;
        self
    }

    /// Build the configuration, validating constraints.
    pub fn build(self) -> Result<ConversionConfig, Pdf2MdError> {
        let c = &self.config;
        if c.dpi < 72 || c.dpi > 400 {
            return Err(Pdf2MdError::InvalidConfig(format!(
                "DPI must be 72–400, got {}",
                c.dpi
            )));
        }
        if c.concurrency == 0 {
            return Err(Pdf2MdError::InvalidConfig(
                "Concurrency must be ≥ 1".into(),
            ));
        }
        Ok(self.config)
    }
}

// ── Enums ────────────────────────────────────────────────────────────────

/// Quality tier controlling which Markdown features the VLM is asked to produce.
///
/// Three tiers exist because prompt complexity trades against cost and latency.
/// Adding LaTeX or HTML-table instructions to the system prompt increases input
/// tokens by ~30 % and may confuse models that are weak at those constructs.
/// Callers can choose the lowest tier that satisfies their downstream needs:
///
/// | Tier | Use case |
/// |------|----------|
/// | 1 | Plain-text extraction, embedding pipelines, sentiment analysis |
/// | 2 | Documentation, wikis, readable reports (default) |
/// | 3 | Scientific papers, technical books with math and complex tables |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FidelityTier {
    /// Basic: text, headings, lists only. Lowest prompt overhead.
    Tier1,
    /// Structural: text, headings, lists, GFM tables, footnotes. (default)
    #[default]
    Tier2,
    /// High-fidelity: Tier2 + LaTeX math (`$…$`, `$$…$$`), HTML table fallback, image captions.
    Tier3,
}

/// Specifies which pages of the PDF to convert.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum PageSelection {
    /// Convert all pages (default).
    #[default]
    All,
    /// Convert a single page (1-indexed).
    Single(usize),
    /// Convert a contiguous range of pages (1-indexed, inclusive).
    Range(usize, usize),
    /// Convert specific pages (1-indexed, deduplicated).
    Set(Vec<usize>),
}

impl PageSelection {
    /// Expand the selection into a sorted, deduplicated list of 0-indexed page numbers.
    pub fn to_indices(&self, total_pages: usize) -> Vec<usize> {
        let mut indices: Vec<usize> = match self {
            PageSelection::All => (0..total_pages).collect(),
            PageSelection::Single(p) => {
                if *p >= 1 && *p <= total_pages {
                    vec![p - 1]
                } else {
                    vec![]
                }
            }
            PageSelection::Range(start, end) => {
                let s = (*start).max(1) - 1;
                let e = (*end).min(total_pages);
                (s..e).collect()
            }
            PageSelection::Set(pages) => pages
                .iter()
                .filter(|&&p| p >= 1 && p <= total_pages)
                .map(|p| p - 1)
                .collect(),
        };
        indices.sort_unstable();
        indices.dedup();
        indices
    }
}

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

impl PageSeparator {
    /// Render the separator string for the given page number (1-indexed).
    pub fn render(&self, page_num: usize) -> String {
        match self {
            PageSeparator::None => "\n\n".to_string(),
            PageSeparator::HorizontalRule => "\n\n---\n\n".to_string(),
            PageSeparator::Comment => format!("\n\n<!-- page {} -->\n\n", page_num),
            PageSeparator::Custom(s) => format!("\n\n{}\n\n", s),
        }
    }
}
