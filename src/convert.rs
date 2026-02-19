//! Eager (full-document) conversion entry points.
//!
//! ## Why eager vs. streaming?
//!
//! This module provides the simpler API: wait for all pages, then return.
//! It collects every [`PageResult`] into memory and assembles the final
//! Markdown document before returning. Use [`crate::stream::convert_stream`]
//! instead when you want pages progressively or need to limit peak memory
//! use on documents with hundreds of pages.

use crate::config::ConversionConfig;
use crate::error::Pdf2MdError;
use crate::output::{ConversionOutput, ConversionStats, DocumentMetadata, PageResult};
use crate::pipeline::{encode, input, llm, postprocess, render};
use edgequake_llm::{LLMProvider, ProviderFactory};
use futures::stream::{self, StreamExt};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, warn};

/// Convert a PDF file or URL to Markdown.
///
/// This is the primary entry point for the library.
///
/// # Arguments
/// * `input` — Local file path or HTTP/HTTPS URL to a PDF
/// * `config` — Conversion configuration
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
pub async fn convert(
    input_str: impl AsRef<str>,
    config: &ConversionConfig,
) -> Result<ConversionOutput, Pdf2MdError> {
    let total_start = Instant::now();
    let input_str = input_str.as_ref();
    info!("Starting conversion: {}", input_str);

    // ── Step 1: Resolve input ────────────────────────────────────────────
    let resolved = input::resolve_input(input_str, config.download_timeout_secs).await?;
    let pdf_path = resolved.path().to_path_buf();

    // ── Step 2: Get/create provider ──────────────────────────────────────
    let provider = resolve_provider(config).await?;

    // ── Step 3: Extract metadata ─────────────────────────────────────────
    let metadata = render::extract_metadata(&pdf_path, config.password.as_deref()).await?;
    let total_pages = metadata.page_count;
    info!("PDF has {} pages", total_pages);

    // ── Step 4: Compute page indices ─────────────────────────────────────
    let page_indices = config.pages.to_indices(total_pages);
    if page_indices.is_empty() {
        return Err(Pdf2MdError::PageOutOfRange {
            page: 0,
            total: total_pages,
        });
    }
    debug!("Selected {} pages for conversion", page_indices.len());

    // Fire on_conversion_start now that we know how many pages will actually
    // be converted (page_indices.len()), not the full document page count.
    if let Some(ref cb) = config.progress_callback {
        cb.on_conversion_start(page_indices.len());
    }

    // ── Step 5: Rasterise pages ──────────────────────────────────────────
    let render_start = Instant::now();
    let rendered = render::render_pages(&pdf_path, config, &page_indices).await?;
    let render_duration_ms = render_start.elapsed().as_millis() as u64;
    info!(
        "Rendered {} pages in {}ms",
        rendered.len(),
        render_duration_ms
    );

    // ── Step 6: Encode images to base64 ──────────────────────────────────
    let encoded: Vec<(usize, _)> = rendered
        .iter()
        .map(|(idx, img)| {
            let data = encode::encode_page(img).map_err(|e| Pdf2MdError::RasterisationFailed {
                page: idx + 1,
                detail: format!("Image encoding failed: {}", e),
            });
            (*idx, data)
        })
        .filter_map(|(idx, result)| match result {
            Ok(data) => Some((idx, data)),
            Err(e) => {
                warn!("Failed to encode page {}: {}", idx + 1, e);
                None
            }
        })
        .collect();

    // ── Step 7: Process pages through VLM ────────────────────────────────
    let llm_start = Instant::now();
    let page_results = if config.maintain_format {
        process_sequential(&provider, &encoded, config).await
    } else {
        process_concurrent(&provider, &encoded, config).await
    };
    let llm_duration_ms = llm_start.elapsed().as_millis() as u64;

    // ── Step 8: Post-process markdown ────────────────────────────────────
    let mut pages: Vec<PageResult> = page_results
        .into_iter()
        .map(|mut pr| {
            if pr.error.is_none() {
                pr.markdown = postprocess::clean_markdown(&pr.markdown);
            }
            pr
        })
        .collect();

    // Sort by page number for consistent output
    pages.sort_by_key(|p| p.page_num);

    // ── Step 9: Assemble final document ──────────────────────────────────
    let markdown = assemble_document(&pages, config, &metadata);

    // ── Step 10: Compute stats ───────────────────────────────────────────
    let processed = pages.iter().filter(|p| p.error.is_none()).count();
    let failed = pages.iter().filter(|p| p.error.is_some()).count();
    let skipped = page_indices.len().saturating_sub(pages.len());

    if processed == 0 {
        let first_error = pages
            .iter()
            .find_map(|p| p.error.as_ref())
            .map(|e| format!("{}", e))
            .unwrap_or_else(|| "Unknown error".to_string());

        return Err(Pdf2MdError::AllPagesFailed {
            total: pages.len(),
            retries: config.max_retries,
            first_error,
        });
    }

    let stats = ConversionStats {
        total_pages,
        processed_pages: processed,
        failed_pages: failed,
        skipped_pages: skipped,
        total_input_tokens: pages.iter().map(|p| p.input_tokens as u64).sum(),
        total_output_tokens: pages.iter().map(|p| p.output_tokens as u64).sum(),
        total_duration_ms: total_start.elapsed().as_millis() as u64,
        render_duration_ms,
        llm_duration_ms,
    };

    info!(
        "Conversion complete: {}/{} pages, {}ms total",
        processed, total_pages, stats.total_duration_ms
    );

    // Fire on_conversion_complete with the count of selected pages, not the
    // full PDF page count, to match what on_conversion_start received.
    if let Some(ref cb) = config.progress_callback {
        cb.on_conversion_complete(page_indices.len(), processed);
    }

    Ok(ConversionOutput {
        markdown,
        pages,
        metadata,
        stats,
    })
}

/// Convert a PDF and write output directly to a file.
///
/// Uses atomic write (temp file + rename) to prevent partial files.
pub async fn convert_to_file(
    input_str: impl AsRef<str>,
    output_path: impl AsRef<Path>,
    config: &ConversionConfig,
) -> Result<ConversionStats, Pdf2MdError> {
    let output = convert(input_str, config).await?;
    let path = output_path.as_ref();

    // Atomic write: write to temp, then rename
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| Pdf2MdError::OutputWriteFailed {
                path: path.to_path_buf(),
                source: e,
            })?;
    }

    let tmp_path = path.with_extension("md.tmp");
    tokio::fs::write(&tmp_path, &output.markdown)
        .await
        .map_err(|e| Pdf2MdError::OutputWriteFailed {
            path: path.to_path_buf(),
            source: e,
        })?;

    tokio::fs::rename(&tmp_path, path)
        .await
        .map_err(|e| Pdf2MdError::OutputWriteFailed {
            path: path.to_path_buf(),
            source: e,
        })?;

    Ok(output.stats)
}

/// Synchronous wrapper around [`convert`].
///
/// Creates a temporary tokio runtime internally.
pub fn convert_sync(
    input_str: impl AsRef<str>,
    config: &ConversionConfig,
) -> Result<ConversionOutput, Pdf2MdError> {
    tokio::runtime::Runtime::new()
        .map_err(|e| Pdf2MdError::Internal(format!("Failed to create tokio runtime: {}", e)))?
        .block_on(convert(input_str, config))
}

/// Extract PDF metadata without converting content.
///
/// Does not require an LLM provider or API key.
pub async fn inspect(input_str: impl AsRef<str>) -> Result<DocumentMetadata, Pdf2MdError> {
    let resolved = input::resolve_input(input_str.as_ref(), 120).await?;
    let pdf_path = resolved.path().to_path_buf();
    render::extract_metadata(&pdf_path, None).await
}

/// Convert PDF bytes in memory to Markdown.
///
/// This avoids the need for the caller to create a temporary file.
/// Internally the library writes `bytes` to a managed [`tempfile`] and cleans
/// it up automatically on return or panic.
///
/// This is the recommended API when PDF data comes from a database, network
/// stream, or in-memory buffer rather than a file on disk.
///
/// # Arguments
/// * `bytes`  — Raw PDF bytes
/// * `config` — Conversion configuration
///
/// # Example
/// ```rust,no_run
/// use edgequake_pdf2md::{convert_from_bytes, ConversionConfig};
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let bytes: Vec<u8> = std::fs::read("document.pdf")?;
/// let config = ConversionConfig::default();
/// let output = convert_from_bytes(&bytes, &config).await?;
/// println!("{}", output.markdown);
/// # Ok(())
/// # }
/// ```
pub async fn convert_from_bytes(
    bytes: &[u8],
    config: &ConversionConfig,
) -> Result<ConversionOutput, Pdf2MdError> {
    let mut tmp = tempfile::NamedTempFile::new()
        .map_err(|e| Pdf2MdError::Internal(format!("tempfile: {e}")))?;
    tmp.write_all(bytes)
        .map_err(|e| Pdf2MdError::Internal(format!("tempfile write: {e}")))?;
    let path = tmp.path().to_string_lossy().to_string();
    // `tmp` is dropped (and the file deleted) when `convert` returns
    convert(&path, config).await
}

// ── Internal helpers ─────────────────────────────────────────────────────

/// Instantiate a named provider with the given model.
///
/// Uses [`ProviderFactory::create_llm_provider`] for all providers.
/// Previously this function routed OpenAI through `OpenAICompatibleProvider`
/// as a workaround for a bug where `OpenAIProvider::convert_messages()` silently
/// dropped `ChatMessage.images`. That bug is fixed in edgequake-llm v0.2.2.
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

/// Resolve the LLM provider, from most-specific to least-specific.
///
/// The four-level fallback chain lets library users and CLI users each set
/// exactly as much or as little as they need:
///
/// 1. **Pre-built provider** (`config.provider`) — the caller constructed and
///    configured the provider entirely; we use it as-is. Useful in tests or
///    when the caller needs custom middleware (caching, rate-limiting).
///
/// 2. **Named provider + model** (`config.provider_name`) — the caller named
///    a provider (e.g. `"openai"`) and optional model. We call
///    [`ProviderFactory::create_llm_provider`] which reads the corresponding
///    API key (`OPENAI_API_KEY`, etc.) from the environment.
///
/// 3. **Environment pair** (`EDGEQUAKE_LLM_PROVIDER` + `EDGEQUAKE_MODEL`) —
///    Both env vars set means the caller chose a provider and model at the
///    execution environment level (Makefile, shell script, CI). Checked before
///    full auto-detection so the model choice is honoured even when multiple
///    API keys are present.
///
/// 4. **Full auto-detection** (`ProviderFactory::from_env`) — the factory
///    scans all known API key variables and picks the first available provider.
///    Convenient for `pdf2md document.pdf` with no other configuration.
async fn resolve_provider(config: &ConversionConfig) -> Result<Arc<dyn LLMProvider>, Pdf2MdError> {
    // 1) User-provided provider takes priority
    if let Some(ref provider) = config.provider {
        return Ok(Arc::clone(provider));
    }

    // 2) Provider name + model
    if let Some(ref name) = config.provider_name {
        let model = config.model.as_deref().unwrap_or("gpt-4.1-nano");
        return create_vision_provider(name, model);
    }

    // 3) Auto-detect from environment; honour EDGEQUAKE_LLM_PROVIDER + EDGEQUAKE_MODEL when both set
    if let (Ok(prov), Ok(model)) = (
        std::env::var("EDGEQUAKE_LLM_PROVIDER"),
        std::env::var("EDGEQUAKE_MODEL"),
    ) {
        if !prov.is_empty() && !model.is_empty() {
            return create_vision_provider(&prov, &model);
        }
    }

    // Prefer OpenAI explicitly when an OpenAI API key is present. This ensures
    // users with multiple provider keys (e.g. Gemini + OpenAI) will default
    // to OpenAI unless they explicitly request another provider.
    if let Ok(openai_key) = std::env::var("OPENAI_API_KEY") {
        if !openai_key.is_empty() {
            let model = config.model.as_deref().unwrap_or("gpt-4.1-nano");
            return create_vision_provider("openai", model);
        }
    }

    let (llm_provider, _embedding) =
        ProviderFactory::from_env().map_err(|e| Pdf2MdError::ProviderNotConfigured {
            provider: "auto".to_string(),
            hint: format!(
                "No LLM provider could be auto-detected from environment.\n\
                Set OPENAI_API_KEY, ANTHROPIC_API_KEY, or configure a provider.\n\
                Error: {}",
                e
            ),
        })?;

    Ok(llm_provider)
}

/// Process pages concurrently (when maintain_format = false).
async fn process_concurrent(
    provider: &Arc<dyn LLMProvider>,
    pages: &[(usize, edgequake_llm::ImageData)],
    config: &ConversionConfig,
) -> Vec<PageResult> {
    let total_pages = pages.len();
    stream::iter(pages.iter().map(|(idx, img_data)| {
        let provider = Arc::clone(provider);
        let page_num = idx + 1;
        let img = img_data.clone();
        let config_clone = config.clone();
        async move {
            if let Some(ref cb) = config_clone.progress_callback {
                cb.on_page_start(page_num, total_pages);
            }
            let result = llm::process_page(&provider, page_num, img, None, &config_clone).await;
            if let Some(ref cb) = config_clone.progress_callback {
                match &result.error {
                    None => cb.on_page_complete(page_num, total_pages, result.markdown.len()),
                    Some(e) => cb.on_page_error(page_num, total_pages, &e.to_string()),
                }
            }
            result
        }
    }))
    .buffer_unordered(config.concurrency)
    .collect()
    .await
}

/// Process pages sequentially, passing each page's markdown as context
/// to the next (when maintain_format = true).
async fn process_sequential(
    provider: &Arc<dyn LLMProvider>,
    pages: &[(usize, edgequake_llm::ImageData)],
    config: &ConversionConfig,
) -> Vec<PageResult> {
    let mut results = Vec::with_capacity(pages.len());
    let mut prior_markdown: Option<String> = None;
    let total_pages = pages.len();

    for (idx, img_data) in pages {
        let page_num = idx + 1;
        if let Some(ref cb) = config.progress_callback {
            cb.on_page_start(page_num, total_pages);
        }
        let result = llm::process_page(
            provider,
            page_num,
            img_data.clone(),
            prior_markdown.as_deref(),
            config,
        )
        .await;

        if let Some(ref cb) = config.progress_callback {
            match &result.error {
                None => cb.on_page_complete(page_num, total_pages, result.markdown.len()),
                Some(e) => cb.on_page_error(page_num, total_pages, &e.to_string()),
            }
        }

        if result.error.is_none() {
            prior_markdown = Some(result.markdown.clone());
        }

        results.push(result);
    }

    results
}

/// Assemble the final markdown document from page results.
fn assemble_document(
    pages: &[PageResult],
    config: &ConversionConfig,
    metadata: &DocumentMetadata,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    // Optional YAML front-matter
    if config.include_metadata {
        parts.push(format_yaml_front_matter(metadata));
    }

    // Collect successful page markdowns
    let successful_pages: Vec<&PageResult> = pages.iter().filter(|p| p.error.is_none()).collect();

    for (i, page) in successful_pages.iter().enumerate() {
        if i > 0 {
            parts.push(config.page_separator.render(page.page_num));
        }
        parts.push(page.markdown.clone());
    }

    parts.join("")
}

/// Format document metadata as YAML front matter.
fn format_yaml_front_matter(meta: &DocumentMetadata) -> String {
    let mut yaml = String::from("---\n");

    if let Some(ref t) = meta.title {
        yaml.push_str(&format!("title: \"{}\"\n", t));
    }
    if let Some(ref a) = meta.author {
        yaml.push_str(&format!("author: \"{}\"\n", a));
    }
    if let Some(ref s) = meta.subject {
        yaml.push_str(&format!("subject: \"{}\"\n", s));
    }
    if let Some(ref c) = meta.creator {
        yaml.push_str(&format!("creator: \"{}\"\n", c));
    }
    if let Some(ref p) = meta.producer {
        yaml.push_str(&format!("producer: \"{}\"\n", p));
    }
    yaml.push_str(&format!("pages: {}\n", meta.page_count));
    if !meta.pdf_version.is_empty() {
        yaml.push_str(&format!("pdf_version: \"{}\"\n", meta.pdf_version));
    }

    yaml.push_str("---\n\n");
    yaml
}
