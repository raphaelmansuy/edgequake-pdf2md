//! # edgequake-pdf2md
//!
//! Convert PDF documents to Markdown using Vision Language Models (VLMs).
//!
//! ## Why this crate?
//!
//! Traditional PDF-to-text tools (pdftotext, pdf-extract) fail on complex
//! layouts — multi-column text, mathematical symbols, figures, and tables come
//! out garbled or out of reading order. Instead this crate rasterises each page
//! into a PNG and lets a VLM read it as a human would, producing semantically
//! correct Markdown that preserves structure, tables, and formulae.
//!
//! ## Pipeline Overview
//!
//! ```text
//! PDF
//!  │
//!  ├─ 1. Input   resolve local file or download from URL
//!  ├─ 2. Render  rasterise pages via pdfium (CPU-bound, spawn_blocking)
//!  ├─ 3. Encode  PNG → base64 ImageData
//!  ├─ 4. VLM     concurrent calls to gpt-4.1-nano / claude / gemini / …
//!  ├─ 5. Polish  10-rule post-processing (fences, tables, whitespace)
//!  └─ 6. Output  assembled Markdown + per-page stats
//! ```
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use edgequake_pdf2md::{convert, ConversionConfig};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Provider auto-detected from OPENAI_API_KEY / ANTHROPIC_API_KEY / GEMINI_API_KEY
//!     let config = ConversionConfig::default();
//!     let output = convert("document.pdf", &config).await?;
//!     println!("{}", output.markdown);
//!     eprintln!("tokens: {} in / {} out",
//!         output.stats.total_input_tokens,
//!         output.stats.total_output_tokens);
//!     Ok(())
//! }
//! ```
//!
//! ## Feature Flags
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `cli`   | on      | Enables the `pdf2md` binary (clap + anyhow + tracing-subscriber) |
//!
//! Disable `cli` when using only the library to avoid pulling in CLI-only deps:
//! ```toml
//! edgequake-pdf2md = { version = "0.1", default-features = false }
//! ```
//!
//! ## Choosing a Model
//!
//! | Model | $/1M tokens | Quality | Best for |
//! |-------|------------|---------|----------|
//! | `gpt-4.1-nano` | $0.10/$0.40 | ★★★ | Default — fast, cheap |
//! | `gpt-4.1-mini` | $0.40/$1.60 | ★★★★ | Balance |
//! | `gpt-4.1`      | $2.00/$8.00 | ★★★★★ | Highest accuracy |
//! | `claude-sonnet-4-20250514` | $3.00/$15.00 | ★★★★★ | Tables, complex layouts |
//! | `gemini-2.0-flash` | $0.10/$0.40 | ★★★ | Alternative cheap option |
//!
//! A 50-page document costs roughly **$0.02** with `gpt-4.1-nano`.

// ── Modules ──────────────────────────────────────────────────────────────

pub mod config;
pub mod convert;
pub mod error;
pub mod output;
pub mod pipeline;
pub mod prompts;
pub mod stream;

// ── Re-exports ───────────────────────────────────────────────────────────

pub use config::{ConversionConfig, ConversionConfigBuilder, FidelityTier, PageSelection, PageSeparator};
pub use convert::{convert, convert_sync, convert_to_file, inspect};
pub use error::{PageError, Pdf2MdError};
pub use output::{ConversionOutput, ConversionStats, DocumentMetadata, PageResult};
pub use stream::convert_stream;
