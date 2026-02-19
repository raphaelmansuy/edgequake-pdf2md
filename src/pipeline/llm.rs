//! VLM interaction: build vision messages and call the provider.
//!
//! This module converts a rasterised page image into a VLM API call and
//! returns structured Markdown. It is intentionally thin — all prompt
//! engineering lives in [`crate::prompts`] so it can be changed without
//! touching retry or error-handling logic here.
//!
//! ## Retry Strategy
//!
//! HTTP 429 / 503 errors from LLM APIs are transient and frequent under
//! concurrent load. Exponential backoff (`retry_backoff_ms * 2^attempt`)
//! avoids thundering-herd: with 500 ms base and 3 retries the wait sequence
//! is 500 ms → 1 s → 2 s, totalling < 4 s of back-off per page.

use crate::config::ConversionConfig;
use crate::output::PageResult;
use crate::prompts::{maintain_format_context, DEFAULT_SYSTEM_PROMPT};
use edgequake_llm::{ChatMessage, CompletionOptions, ImageData, LLMProvider};
use std::sync::Arc;
use std::time::Instant;
use tokio::time::{sleep, Duration};
use tracing::{debug, warn};

/// Convert a single rasterised page into Markdown via the VLM.
///
/// ## Message Layout
///
/// The request contains (in order):
/// 1. **System message** — the 7-rule conversion prompt (or user-supplied override)
/// 2. **Format-continuity message** *(maintain_format only)* — previous page markdown
///    as context so the VLM keeps numbering, style, and running text consistent
/// 3. **User message** — the page PNG as a base64 image attachment (empty text)
///
/// The empty user text is intentional: VLM APIs require at least one user
/// turn to respond to, but the image carries all the actual content.
///
/// ## Return Value
///
/// Always returns a `PageResult` — never propagates the error upward so a
/// single bad page doesn't abort the entire document. Callers check
/// `result.error` to decide whether to include or skip the page.
pub async fn process_page(
    provider: &Arc<dyn LLMProvider>,
    page_num: usize,
    image_data: ImageData,
    prior_page: Option<&str>,
    config: &ConversionConfig,
) -> PageResult {
    let start = Instant::now();
    let system_prompt = config
        .system_prompt
        .as_deref()
        .unwrap_or(DEFAULT_SYSTEM_PROMPT);

    let mut messages = vec![ChatMessage::system(system_prompt)];

    // Maintain format context from prior page
    if config.maintain_format {
        if let Some(prior) = prior_page {
            if !prior.is_empty() {
                messages.push(ChatMessage::system(maintain_format_context(prior)));
            }
        }
    }

    // User message with the page image
    messages.push(ChatMessage::user_with_images(
        "",
        vec![image_data],
    ));

    let options = build_options(config);

    let mut last_err: Option<String> = None;

    for attempt in 0..=config.max_retries {
        if attempt > 0 {
            let backoff = config.retry_backoff_ms * 2u64.pow(attempt - 1);
            warn!(
                "Page {}: retry {}/{} after {}ms",
                page_num, attempt, config.max_retries, backoff
            );
            sleep(Duration::from_millis(backoff)).await;
        }

        match provider.chat(&messages, Some(&options)).await {
            Ok(response) => {
                let duration = start.elapsed();
                debug!(
                    "Page {}: {} input tokens, {} output tokens, {:?}",
                    page_num,
                    response.prompt_tokens,
                    response.completion_tokens,
                    duration
                );

                return PageResult {
                    page_num,
                    markdown: response.content,
                    input_tokens: response.prompt_tokens,
                    output_tokens: response.completion_tokens,
                    duration_ms: duration.as_millis() as u64,
                    retries: attempt as u8,
                    error: None,
                };
            }
            Err(e) => {
                let err_msg = format!("{}", e);
                warn!("Page {}: attempt {} failed — {}", page_num, attempt + 1, err_msg);
                last_err = Some(err_msg);
            }
        }
    }

    // All retries exhausted
    let duration = start.elapsed();
    let err_msg = last_err.unwrap_or_else(|| "Unknown error".to_string());

    PageResult {
        page_num,
        markdown: String::new(),
        input_tokens: 0,
        output_tokens: 0,
        duration_ms: duration.as_millis() as u64,
        retries: config.max_retries as u8,
        error: Some(crate::error::PageError::LlmFailed {
            page: page_num,
            retries: config.max_retries as u8,
            detail: err_msg,
        }),
    }
}

/// Build `CompletionOptions` from the conversion config.
fn build_options(config: &ConversionConfig) -> CompletionOptions {
    CompletionOptions {
        temperature: Some(config.temperature),
        max_tokens: Some(config.max_tokens),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_options_defaults() {
        let config = ConversionConfig::default();
        let opts = build_options(&config);
        assert_eq!(opts.temperature, Some(0.1));
        assert_eq!(opts.max_tokens, Some(4096));
    }
}
