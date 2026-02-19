//! Streaming conversion API: emit pages as they complete.
//!
//! ## Why stream?
//!
//! Large documents take minutes. A streams-based API lets callers display
//! partial results immediately, wire up progress bars, or write pages to disk
//! incrementally instead of buffering the entire document in memory.
//!
//! Unlike the eager [`crate::convert::convert`] which returns only after
//! all pages finish, [`convert_stream`] yields `PageResult` items via a
//! `Stream` as each page completes. In concurrent mode pages may arrive out
//! of order (sort by `page_num` if order matters).

use crate::config::ConversionConfig;
use crate::error::{PageError, Pdf2MdError};
use crate::output::PageResult;
use crate::pipeline::{encode, input, llm, postprocess, render};
use edgequake_llm::{LLMProvider, ProviderFactory};
use futures::stream::{self, StreamExt};
use std::io::Write;
use std::pin::Pin;
use std::sync::Arc;
use tokio_stream::Stream;
use tracing::{info, warn};

/// A boxed stream of page results.
pub type PageStream = Pin<Box<dyn Stream<Item = Result<PageResult, PageError>> + Send>>;

/// Convert a PDF to Markdown, streaming pages as they are ready.
///
/// Pages are emitted in completion order (not necessarily page order)
/// when `maintain_format = false`. Sort by `page_num` if order matters.
///
/// # Returns
/// - `Ok(PageStream)` — a stream of `Result<PageResult, PageError>`
/// - `Err(Pdf2MdError)` — fatal error (file not found, not a PDF, etc.)
pub async fn convert_stream(
    input_str: impl AsRef<str>,
    config: &ConversionConfig,
) -> Result<PageStream, Pdf2MdError> {
    let input_str = input_str.as_ref();
    info!("Starting streaming conversion: {}", input_str);

    // ── Resolve input ────────────────────────────────────────────────────
    let resolved = input::resolve_input(input_str, config.download_timeout_secs).await?;
    let pdf_path = resolved.path().to_path_buf();

    // ── Get provider ─────────────────────────────────────────────────────
    let provider = resolve_provider(config)?;

    // ── Extract metadata for page count ──────────────────────────────────
    let metadata = render::extract_metadata(&pdf_path, config.password.as_deref()).await?;
    let total_pages = metadata.page_count;

    // ── Compute page indices ─────────────────────────────────────────────
    let page_indices = config.pages.to_indices(total_pages);
    if page_indices.is_empty() {
        return Err(Pdf2MdError::PageOutOfRange {
            page: 0,
            total: total_pages,
        });
    }

    // ── Render all pages ─────────────────────────────────────────────────
    let rendered = render::render_pages(&pdf_path, config, &page_indices).await?;

    // ── Encode images ────────────────────────────────────────────────────
    let encoded: Vec<(usize, edgequake_llm::ImageData)> = rendered
        .iter()
        .filter_map(|(idx, img)| match encode::encode_page(img) {
            Ok(data) => Some((*idx, data)),
            Err(e) => {
                warn!("Failed to encode page {}: {}", idx + 1, e);
                None
            }
        })
        .collect();

    // ── Build the stream ─────────────────────────────────────────────────
    let concurrency = config.concurrency;
    let config_clone = config.clone();

    if config.maintain_format {
        // Sequential mode: must process in order
        let s = stream::iter(encoded.into_iter()).then(move |(idx, img_data)| {
            let provider = Arc::clone(&provider);
            let cfg = config_clone.clone();
            async move {
                let page_num = idx + 1;
                let mut result = llm::process_page(&provider, page_num, img_data, None, &cfg).await;
                if result.error.is_none() {
                    result.markdown = postprocess::clean_markdown(&result.markdown);
                    Ok(result)
                } else {
                    let err = result.error.take().unwrap();
                    Err(err)
                }
            }
        });

        Ok(Box::pin(s))
    } else {
        // Concurrent mode: process in parallel, emit as ready
        let s = stream::iter(encoded.into_iter().map(move |(idx, img_data)| {
            let provider = Arc::clone(&provider);
            let cfg = config_clone.clone();
            async move {
                let page_num = idx + 1;
                let mut result = llm::process_page(&provider, page_num, img_data, None, &cfg).await;
                if result.error.is_none() {
                    result.markdown = postprocess::clean_markdown(&result.markdown);
                    Ok(result)
                } else {
                    let err = result.error.take().unwrap();
                    Err(err)
                }
            }
        }))
        .buffer_unordered(concurrency);

        Ok(Box::pin(s))
    }
}

/// Convert PDF bytes in memory to Markdown, streaming pages as they complete.
///
/// This is the streaming equivalent of [`crate::convert::convert_from_bytes`].
/// The PDF bytes are written to a temporary file internally; the file is cleaned
/// up automatically when the returned stream (and all its futures) are dropped.
///
/// # Arguments
/// * `bytes`  — Raw PDF bytes
/// * `config` — Conversion configuration
///
/// # Returns
/// - `Ok(PageStream)` — a stream of `Result<PageResult, PageError>`
/// - `Err(Pdf2MdError)` — fatal error (not a PDF, provider not configured, etc.)
///
/// # Example
/// ```rust,no_run
/// use edgequake_pdf2md::{convert_stream_from_bytes, ConversionConfig};
/// use futures::StreamExt;
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let bytes: Vec<u8> = std::fs::read("document.pdf")?;
/// let config = ConversionConfig::default();
/// let mut stream = convert_stream_from_bytes(&bytes, &config).await?;
/// while let Some(page) = stream.next().await {
///     match page {
///         Ok(p) => println!("Page {}: {} chars", p.page_num, p.markdown.len()),
///         Err(e) => eprintln!("Error: {e}"),
///     }
/// }
/// # Ok(())
/// # }
/// ```
pub async fn convert_stream_from_bytes(
    bytes: &[u8],
    config: &ConversionConfig,
) -> Result<PageStream, Pdf2MdError> {
    let mut tmp = tempfile::NamedTempFile::new()
        .map_err(|e| Pdf2MdError::Internal(format!("tempfile: {e}")))?;
    tmp.write_all(bytes)
        .map_err(|e| Pdf2MdError::Internal(format!("tempfile write: {e}")))?;
    let path = tmp.path().to_string_lossy().to_string();
    // Keep `tmp` alive for the duration of this call; the stream is fully
    // materialised (pages rendered + encoded) before we return, so it is safe
    // to drop the tempfile here.
    let stream = convert_stream(&path, config).await?;
    drop(tmp);
    Ok(stream)
}

/// Resolve LLM provider from config.
fn resolve_provider(config: &ConversionConfig) -> Result<Arc<dyn LLMProvider>, Pdf2MdError> {
    if let Some(ref provider) = config.provider {
        return Ok(Arc::clone(provider));
    }

    if let Some(ref name) = config.provider_name {
        let model = config.model.as_deref().unwrap_or("gpt-4.1-nano");
        return create_vision_provider(name, model);
    }

    // Honour EDGEQUAKE_LLM_PROVIDER + EDGEQUAKE_MODEL when both set
    if let (Ok(prov), Ok(model)) = (
        std::env::var("EDGEQUAKE_LLM_PROVIDER"),
        std::env::var("EDGEQUAKE_MODEL"),
    ) {
        if !prov.is_empty() && !model.is_empty() {
            return create_vision_provider(&prov, &model);
        }
    }

    // Prefer OpenAI explicitly when an OpenAI API key is present.
    // This ensures users with multiple provider keys (e.g. Gemini + OpenAI)
    // will default to OpenAI unless they explicitly request another provider.
    if let Ok(openai_key) = std::env::var("OPENAI_API_KEY") {
        if !openai_key.is_empty() {
            let model = config.model.as_deref().unwrap_or("gpt-4.1-nano");
            return create_vision_provider("openai", model);
        }
    }

    let (llm_provider, _) =
        ProviderFactory::from_env().map_err(|e| Pdf2MdError::ProviderNotConfigured {
            provider: "auto".to_string(),
            hint: format!("No LLM provider auto-detected: {}", e),
        })?;

    Ok(llm_provider)
}

/// Instantiate a named provider with the given model.
///
/// Uses [`ProviderFactory::create_llm_provider`] uniformly for all providers.
/// Previously OpenAI was routed through `OpenAICompatibleProvider` to work around
/// a bug in `OpenAIProvider::convert_messages()` that silently dropped image data.
/// That bug was fixed upstream in edgequake-llm v0.2.2.
fn create_vision_provider(
    provider_name: &str,
    model: &str,
) -> Result<Arc<dyn LLMProvider>, Pdf2MdError> {
    ProviderFactory::create_llm_provider(provider_name, model).map_err(|e| {
        Pdf2MdError::ProviderNotConfigured {
            provider: provider_name.to_string(),
            hint: format!("{e}"),
        }
    })
}
