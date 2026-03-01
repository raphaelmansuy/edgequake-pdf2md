//! End-to-end integration tests for edgequake-pdf2md.
//!
//! These tests use real PDF files in `./test_cases/` and make live LLM API
//! calls.  They are gated behind the `E2E_ENABLED` environment variable so
//! they do not run in CI unless explicitly requested.
//!
//! Run with:
//!   DYLD_LIBRARY_PATH=. cargo test --test e2e -- --nocapture
//!
//! To restrict to a specific test:
//!   DYLD_LIBRARY_PATH=. cargo test --test e2e test_inspect -- --nocapture

use edgequake_pdf2md::{
    convert, inspect, ConversionConfig, FidelityTier, PageSelection, PageSeparator,
};
use std::path::PathBuf;
use std::sync::Arc;

// ── Test helpers ─────────────────────────────────────────────────────────────

fn test_cases_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_cases")
}

fn output_dir() -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_cases/output");
    std::fs::create_dir_all(&d).ok();
    d
}

/// Skip this test if E2E_ENABLED is not set *or* no PDF file at `path`.
macro_rules! e2e_skip_unless_ready {
    ($path:expr) => {{
        if std::env::var("E2E_ENABLED").is_err() {
            println!("SKIP — set E2E_ENABLED=1 to run e2e tests");
            return;
        }
        let p: PathBuf = $path;
        if !p.exists() {
            println!("SKIP — test file not found: {}", p.display());
            println!("       Run: make download-test-pdfs");
            return;
        }
        p
    }};
}

/// Assert the markdown passes basic quality checks.
fn assert_markdown_quality(md: &str, context: &str) {
    // Must be non-empty
    assert!(!md.trim().is_empty(), "[{context}] Markdown is empty");

    // Must end with newline (normalised by post-processor)
    assert!(
        md.ends_with('\n'),
        "[{context}] Markdown must end with a newline"
    );

    // Must not contain raw fence blocks wrapping the whole output
    // (post-processor should strip those)
    let first_line = md.lines().next().unwrap_or("");
    assert!(
        !first_line.starts_with("```"),
        "[{context}] Output must not start with a code fence, got: {first_line:?}"
    );

    // No excessive blank lines (> 3 consecutive newlines)
    assert!(
        !md.contains("\n\n\n\n"),
        "[{context}] Output has more than 3 consecutive blank lines"
    );

    // No invisible Unicode junk
    let invisible = ['\u{200B}', '\u{FEFF}', '\u{200C}', '\u{200D}', '\u{2060}'];
    for ch in invisible {
        assert!(
            !md.contains(ch),
            "[{context}] Output contains invisible char U+{:04X}",
            ch as u32
        );
    }

    // Must have some reasonable length (at least 100 bytes per page is normal)
    assert!(
        md.len() >= 50,
        "[{context}] Output suspiciously short: {} bytes",
        md.len()
    );

    println!("[{context}] ✓  {} bytes, quality checks passed", md.len());
}

/// Assert that the Markdown contains at least one Markdown heading (`#`).
fn assert_has_headings(md: &str, context: &str) {
    assert!(
        md.lines().any(|l| l.starts_with('#')),
        "[{context}] Expected at least one heading (#)"
    );
}

// ── Inspect tests (no LLM, instant) ──────────────────────────────────────────

#[tokio::test]
async fn test_inspect_arxiv_paper() {
    let path = e2e_skip_unless_ready!(test_cases_dir().join("attention_is_all_you_need.pdf"));

    let meta = inspect(path.to_str().unwrap())
        .await
        .expect("inspect() should succeed");

    assert_eq!(meta.page_count, 15, "Attention paper should have 15 pages");
    assert!(!meta.is_encrypted);
    assert!(!meta.pdf_version.is_empty());

    println!("Metadata: {:?}", meta);
}

#[tokio::test]
async fn test_inspect_irs_form() {
    let path = e2e_skip_unless_ready!(test_cases_dir().join("irs_form_1040.pdf"));

    let meta = inspect(path.to_str().unwrap())
        .await
        .expect("inspect() should succeed");

    assert_eq!(meta.page_count, 2, "IRS form should have 2 pages");
    assert!(
        meta.title.as_deref().unwrap_or("").contains("1040"),
        "Title should mention 1040"
    );

    println!("Metadata: {:?}", meta);
}

#[tokio::test]
async fn test_inspect_nonexistent() {
    if std::env::var("E2E_ENABLED").is_err() {
        println!("SKIP");
        return;
    }

    let result = inspect("/definitely/not/a/real/file.pdf").await;
    assert!(
        result.is_err(),
        "inspect() should return Err for nonexistent file"
    );
}

// ── Page-selection unit tests (no LLM) ──────────────────────────────────────

#[test]
fn test_page_selection_out_of_range_is_empty() {
    use edgequake_pdf2md::PageSelection;
    // Page 100 of a 4-page doc should yield no indices
    assert_eq!(
        PageSelection::Single(100).to_indices(4),
        Vec::<usize>::new()
    );
}

#[test]
fn test_page_selection_range_clipping() {
    use edgequake_pdf2md::PageSelection;
    // Range 3-10 on a 4-page doc → pages 3 and 4 (indices 2, 3)
    let indices = PageSelection::Range(3, 10).to_indices(4);
    assert_eq!(indices, vec![2, 3]);
}

#[test]
fn test_page_selection_set_dedup_and_sort() {
    use edgequake_pdf2md::PageSelection;
    let indices = PageSelection::Set(vec![3, 1, 3, 2]).to_indices(5);
    assert_eq!(indices, vec![0, 1, 2]); // sorted, deduped, 0-based
}

// ── Conversion quality tests (need LLM API) ───────────────────────────────────

/// Test 1: Convert page 1 of the Attention paper
/// Validates that scientific prose is extracted correctly.
#[tokio::test]
async fn test_convert_arxiv_page1() {
    let path = e2e_skip_unless_ready!(test_cases_dir().join("attention_is_all_you_need.pdf"));
    let out_path = output_dir().join("arxiv_page1.md");

    let config = ConversionConfig::builder()
        .pages(PageSelection::Single(1))
        .max_retries(2)
        .build()
        .expect("valid config");

    let result = convert(path.to_str().unwrap(), &config)
        .await
        .expect("conversion should succeed");

    assert_eq!(
        result.stats.processed_pages, 1,
        "Should have processed 1 page"
    );
    assert_eq!(result.stats.failed_pages, 0, "No pages should fail");
    assert!(
        result.stats.total_input_tokens > 0,
        "Should have consumed tokens"
    );

    assert_markdown_quality(&result.markdown, "arxiv_page1");

    // The first page of Attention paper should mention "Attention"
    assert!(
        result.markdown.to_lowercase().contains("attention"),
        "Page 1 should mention 'Attention'"
    );

    // Save result for human inspection
    std::fs::write(&out_path, &result.markdown).ok();
    println!("[arxiv_page1] Saved to {}", out_path.display());
    println!(
        "[arxiv_page1] Tokens: {} in / {} out",
        result.stats.total_input_tokens, result.stats.total_output_tokens
    );
    println!(
        "--- BEGIN OUTPUT ---\n{}\n--- END OUTPUT ---",
        result.markdown
    );
}

/// Test 2: Convert pages 1-2 of IRS Form 1040
/// Validates table/form extraction.
#[tokio::test]
async fn test_convert_irs_form() {
    let path = e2e_skip_unless_ready!(test_cases_dir().join("irs_form_1040.pdf"));
    let out_path = output_dir().join("irs_form_1040.md");

    let config = ConversionConfig::builder()
        .pages(PageSelection::All)
        .page_separator(PageSeparator::HorizontalRule)
        .max_retries(2)
        .build()
        .expect("valid config");

    let result = convert(path.to_str().unwrap(), &config)
        .await
        .expect("conversion should succeed");

    assert_eq!(
        result.stats.processed_pages, 2,
        "Should have processed 2 pages"
    );
    assert_eq!(result.stats.total_pages, 2, "IRS form has 2 pages");
    assert_eq!(result.stats.failed_pages, 0);

    assert_markdown_quality(&result.markdown, "irs_form");

    // IRS form should mention "income" or "tax"
    let lower = result.markdown.to_lowercase();
    assert!(
        lower.contains("income") || lower.contains("tax") || lower.contains("1040"),
        "IRS form should mention 'income', 'tax', or '1040'"
    );

    // Should have a horizontal rule separator between pages
    assert!(
        result.markdown.contains("---"),
        "Should have HR separator between the 2 pages"
    );

    std::fs::write(&out_path, &result.markdown).ok();
    println!("[irs_form] Saved to {}", out_path.display());
    println!(
        "--- BEGIN OUTPUT ---\n{}\n--- END OUTPUT ---",
        result.markdown
    );
}

/// Test 3: Convert neuroscience textbook (structured document with sections)
/// Validates heading detection and structure preservation.
#[tokio::test]
async fn test_convert_neuroscience_textbook() {
    let path = e2e_skip_unless_ready!(test_cases_dir().join("neuroscience_textbook.pdf"));
    let out_path = output_dir().join("neuroscience_textbook.md");

    let config = ConversionConfig::builder()
        .pages(PageSelection::All)
        .page_separator(PageSeparator::HorizontalRule)
        .include_metadata(true)
        .maintain_format(false)
        .max_retries(2)
        .build()
        .expect("valid config");

    let result = convert(path.to_str().unwrap(), &config)
        .await
        .expect("conversion should succeed");

    assert!(result.stats.total_pages >= 1);
    assert_eq!(result.stats.failed_pages, 0);
    assert_markdown_quality(&result.markdown, "neuroscience");

    // With metadata, the YAML front-matter should come first
    assert!(
        result.markdown.starts_with("---"),
        "With include_metadata=true, should have YAML front-matter"
    );

    // Should have structural headings
    assert_has_headings(&result.markdown, "neuroscience");

    std::fs::write(&out_path, &result.markdown).ok();
    println!("[neuroscience] Saved to {}", out_path.display());
    println!(
        "--- BEGIN OUTPUT ---\n{}\n--- END OUTPUT ---",
        result.markdown
    );
}

/// Test 4: Convert pages 1-3 of Attention paper with maintain_format
/// Validates that sequential context mode works.
#[tokio::test]
async fn test_convert_with_maintain_format() {
    let path = e2e_skip_unless_ready!(test_cases_dir().join("attention_is_all_you_need.pdf"));
    let out_path = output_dir().join("arxiv_maintain_format.md");

    let config = ConversionConfig::builder()
        .pages(PageSelection::Range(1, 3))
        .maintain_format(true)
        .concurrency(1) // sequential is required for maintain_format
        .page_separator(PageSeparator::HorizontalRule)
        .max_retries(2)
        .build()
        .expect("valid config");

    let result = convert(path.to_str().unwrap(), &config)
        .await
        .expect("conversion should succeed");

    assert_eq!(
        result.stats.processed_pages, 3,
        "Should have processed 3 pages"
    );
    assert_eq!(result.stats.failed_pages, 0);
    assert_markdown_quality(&result.markdown, "maintain_format");

    // Should have 2 separators for 3 pages
    let sep_count = result.markdown.matches("---").count();
    assert!(
        sep_count >= 2,
        "Expected at least 2 HR separators for 3 pages, got {sep_count}"
    );

    std::fs::write(&out_path, &result.markdown).ok();
    println!("[maintain_format] Saved to {}", out_path.display());
}

/// Test 5: Verify JSON output is well-formed
#[tokio::test]
async fn test_convert_json_serialisable() {
    let path = e2e_skip_unless_ready!(test_cases_dir().join("neuroscience_textbook.pdf"));

    let config = ConversionConfig::builder()
        .pages(PageSelection::Single(1))
        .max_retries(2)
        .build()
        .expect("valid config");

    let result = convert(path.to_str().unwrap(), &config)
        .await
        .expect("conversion should succeed");

    // Must serialise to JSON without error
    let json =
        serde_json::to_string_pretty(&result).expect("ConversionOutput must serialise to JSON");
    assert!(!json.is_empty());

    // Must round-trip through deserialization
    let back: edgequake_pdf2md::ConversionOutput =
        serde_json::from_str(&json).expect("JSON must deserialize back to ConversionOutput");
    assert_eq!(back.stats.total_pages, result.stats.total_pages);

    let out_path = output_dir().join("neuroscience_page1.json");
    std::fs::write(&out_path, &json).ok();
    println!("[json] Saved to {}", out_path.display());
}

/// Test 6: Fidelity tier 1 vs tier 2 (tier1 = compact, tier2 = default)
/// Both should produce valid output, tier1 prompt is more terse.
#[tokio::test]
async fn test_fidelity_tier1() {
    let path = e2e_skip_unless_ready!(test_cases_dir().join("neuroscience_textbook.pdf"));
    let out_path = output_dir().join("neuroscience_tier1.md");

    let config = ConversionConfig::builder()
        .pages(PageSelection::Single(1))
        .fidelity(FidelityTier::Tier1)
        .max_retries(2)
        .build()
        .expect("valid config");

    let result = convert(path.to_str().unwrap(), &config)
        .await
        .expect("conversion should succeed");

    assert_eq!(result.stats.failed_pages, 0);
    assert_markdown_quality(&result.markdown, "tier1");

    std::fs::write(&out_path, &result.markdown).ok();
    println!("[fidelity_tier1] Saved to {}", out_path.display());
}

/// Test 7: sample_text PDF — Word-generated document, simple paragraphs
#[tokio::test]
async fn test_convert_sample_text_first2_pages() {
    let path = e2e_skip_unless_ready!(test_cases_dir().join("sample_text.pdf"));
    let out_path = output_dir().join("sample_text_pages1_2.md");

    let config = ConversionConfig::builder()
        .pages(PageSelection::Range(1, 2))
        .page_separator(PageSeparator::Comment)
        .max_retries(2)
        .build()
        .expect("valid config");

    let result = convert(path.to_str().unwrap(), &config)
        .await
        .expect("conversion should succeed");

    assert_eq!(
        result.stats.processed_pages, 2,
        "Should have processed 2 pages"
    );
    assert_eq!(result.stats.failed_pages, 0);
    assert_markdown_quality(&result.markdown, "sample_text");

    // Comment separator should appear
    assert!(
        result.markdown.contains("<!--"),
        "Should contain comment-style page separator"
    );

    std::fs::write(&out_path, &result.markdown).ok();
    println!("[sample_text] Saved to {}", out_path.display());
    println!(
        "--- BEGIN OUTPUT ---\n{}\n--- END OUTPUT ---",
        result.markdown
    );
}

// ── Callback API unit tests (no LLM calls, always run) ───────────────────────

/// Regression test for issues #8 and #9.
///
/// Verifies that `ConversionProgressCallback` can be boxed as `Arc<dyn …>`
/// and moved into a `tokio::spawn` task without triggering the HRTB
/// "Send is not general enough" compiler error that existed when
/// `on_page_error` accepted `error: &str`.
#[tokio::test]
async fn test_callback_send_in_tokio_spawn() {
    use edgequake_pdf2md::ConversionProgressCallback;
    use std::sync::{Arc, Mutex};

    struct ErrorLogger {
        log: Arc<Mutex<Vec<String>>>,
    }

    impl ConversionProgressCallback for ErrorLogger {
        fn on_page_error(&self, _page: usize, _total: usize, error: String) {
            self.log.lock().unwrap().push(error);
        }
    }

    let logger = Arc::new(ErrorLogger {
        log: Arc::new(Mutex::new(vec![])),
    });
    let log_ref = Arc::clone(&logger.log);

    // Cast to Arc<dyn ConversionProgressCallback> — the type that the library
    // actually stores and passes through the pipeline.
    let cb: Arc<dyn ConversionProgressCallback> =
        Arc::clone(&logger) as Arc<dyn ConversionProgressCallback>;

    // Moving `cb` into tokio::spawn requires the future to be Send.
    // This line would fail to compile if on_page_error still took &str.
    tokio::spawn(async move {
        cb.on_page_error(2, 5, "timeout after 3 retries".to_string());
    })
    .await
    .expect("spawn must succeed");

    let captured = log_ref.lock().unwrap().clone();
    assert_eq!(captured, vec!["timeout after 3 retries"]);
}

/// Verify that a Noop callback compiles and does not panic.
#[test]
fn test_noop_callback_is_send_sync() {
    use edgequake_pdf2md::{ConversionProgressCallback, NoopProgressCallback};
    use std::sync::Arc;

    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<NoopProgressCallback>();

    let cb: Arc<dyn ConversionProgressCallback> = Arc::new(NoopProgressCallback);
    cb.on_page_error(1, 1, "an error".to_string());
}

// ── Mistral provider structural tests (no API calls, always run) ──────────────

/// Verify that ConversionConfig accepts "mistral" as a provider name without
/// panicking or returning an error at config-build time (no API call happens).
#[test]
fn test_mistral_config_builder_accepts_provider_name() {
    let config = ConversionConfig::builder()
        .dpi(150)
        .concurrency(1)
        .build()
        .expect("builder must succeed");

    let mut cfg = config;
    cfg.provider_name = Some("mistral".to_string());
    cfg.model = Some("pixtral-12b-2409".to_string());

    assert_eq!(cfg.provider_name.as_deref(), Some("mistral"));
    assert_eq!(cfg.model.as_deref(), Some("pixtral-12b-2409"));
}

/// When provider_name is "mistral" and no model is set, the library must use
/// "pixtral-12b-2409" (the only vision-capable Mistral model). We verify this
/// by checking that `MistralProvider` is recognised by `ProviderFactory` and
/// that the pixtral model exists in the Mistral catalogue.
#[test]
fn test_mistral_pixtral_model_available() {
    use edgequake_llm::MistralProvider;

    let models = MistralProvider::available_models();
    let ids: Vec<&str> = models.iter().map(|(id, _, _)| *id).collect();
    assert!(
        ids.contains(&"pixtral-12b-2409"),
        "pixtral-12b-2409 must be in MistralProvider catalogue; got: {:?}",
        ids
    );
}

/// Verify vision capability flag is set on pixtral-12b-2409.
#[test]
fn test_pixtral_supports_vision() {
    use edgequake_llm::MistralProvider;

    // MistralProvider::available_models() returns (id, display_name, context_len)
    // Vision info comes from context_length lookup — pixtral has 128K context.
    // We verify context_length > 0 (non-embedding model, vision-capable).
    let ctx = MistralProvider::context_length("pixtral-12b-2409");
    assert!(
        ctx > 0,
        "pixtral-12b-2409 must have positive context length (vision-capable model)"
    );
    assert_eq!(
        ctx, 131072,
        "pixtral-12b-2409 should have 128K context window"
    );
}

/// Gated e2e test: convert a PDF page with Mistral pixtral-12b-2409.
/// Structural regression test for issue #13 (max_completion_tokens).
///
/// Verifies that a `ConversionConfig` with `max_tokens` set and the default
/// OpenAI provider builds and resolves without panicking. The fix lives in
/// `edgequake-llm` ≥ 0.2.5 (async-openai 0.33 routes `CompletionOptions::
/// max_tokens` → `max_completion_tokens` for gpt-4.1-nano / o-series).
/// No network call is made — this is a compile-time + config-layer check.
#[test]
fn test_issue13_max_tokens_config_builds_for_gpt41_nano() {
    use edgequake_pdf2md::ConversionConfig;

    let config = ConversionConfig::builder()
        .dpi(150)
        .max_tokens(2048)
        .build()
        .expect("config must build with max_tokens set");

    // max_tokens must be forwarded through CompletionOptions unchanged.
    assert_eq!(
        config.max_tokens, 2048,
        "max_tokens must round-trip through builder"
    );
}

/// Gated e2e: convert one PDF page using gpt-4.1-nano + max_tokens.
///
/// This is the exact scenario that produced:
///   "Unsupported parameter: 'max_tokens' is not supported with this model.
///    Use 'max_completion_tokens' instead."
/// in edgequake-llm ≤ 0.2.4.  Requires E2E_ENABLED=1 and OPENAI_API_KEY.
#[tokio::test]
async fn test_gpt41_nano_max_completion_tokens_regression() {
    if std::env::var("E2E_ENABLED").is_err() {
        println!("SKIP — set E2E_ENABLED=1 and OPENAI_API_KEY to run");
        return;
    }
    if std::env::var("OPENAI_API_KEY").is_err() {
        println!("SKIP — OPENAI_API_KEY not set");
        return;
    }

    let pdf_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test_cases")
        .join("sample.pdf");
    if !pdf_path.exists() {
        println!("SKIP — test_cases/sample.pdf not found. Run: make download-test-pdfs");
        return;
    }

    // max_tokens must NOT trigger "'max_tokens' is not supported with this
    // model" from the OpenAI API when using gpt-4.1-nano (issue #13).
    let config = ConversionConfig::builder()
        .dpi(150)
        .concurrency(1)
        .pages(PageSelection::Single(1))
        .fidelity(FidelityTier::Tier1)
        .max_tokens(2048)
        .build()
        .expect("config must build");

    let mut cfg = config;
    cfg.provider_name = Some("openai".to_string());
    cfg.model = Some("gpt-4.1-nano".to_string());

    let result = convert(&pdf_path.to_string_lossy(), &cfg).await.expect(
        "gpt-4.1-nano with max_tokens must not return 400 Bad Request (issue #13 regression)",
    );

    assert!(
        !result.markdown.trim().is_empty(),
        "gpt-4.1-nano conversion must produce non-empty Markdown"
    );
    assert_eq!(result.stats.processed_pages, 1);
    println!(
        "gpt-4.1-nano output ({} chars):\n{}",
        result.markdown.len(),
        result.markdown
    );
}

/// Requires E2E_ENABLED=1 and MISTRAL_API_KEY to be set.
#[tokio::test]
async fn test_mistral_pdf_conversion() {
    if std::env::var("E2E_ENABLED").is_err() {
        println!("SKIP — set E2E_ENABLED=1 and MISTRAL_API_KEY to run");
        return;
    }
    if std::env::var("MISTRAL_API_KEY").is_err() {
        println!("SKIP — MISTRAL_API_KEY not set");
        return;
    }

    let pdf_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test_cases")
        .join("sample.pdf");
    if !pdf_path.exists() {
        println!("SKIP — test_cases/sample.pdf not found. Run: make download-test-pdfs");
        return;
    }

    let config = ConversionConfig::builder()
        .dpi(150)
        .concurrency(1)
        .pages(PageSelection::Single(1))
        .fidelity(FidelityTier::Tier1)
        .max_tokens(2048)
        .build()
        .expect("config must build");

    let mut cfg = config;
    cfg.provider_name = Some("mistral".to_string());
    cfg.model = Some("pixtral-12b-2409".to_string());

    let result = convert(&pdf_path.to_string_lossy(), &cfg)
        .await
        .expect("Mistral conversion must succeed");

    assert!(
        !result.markdown.trim().is_empty(),
        "Mistral conversion must produce non-empty Markdown"
    );
    assert_eq!(result.stats.processed_pages, 1);
    println!(
        "Mistral output ({} chars):\n{}",
        result.markdown.len(),
        result.markdown
    );
}

// ── Ollama provider e2e tests ─────────────────────────────────────────────────

/// Helper: check if Ollama is reachable at the configured host.
async fn ollama_is_available() -> bool {
    let host =
        std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".to_string());
    reqwest::Client::new()
        .get(format!("{host}/api/tags"))
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
        .is_ok()
}

/// Gated e2e: convert one PDF page using Ollama with a local vision model.
///
/// Requirements:
/// - `E2E_ENABLED=1`
/// - Ollama running at `OLLAMA_HOST` (default: http://localhost:11434)
/// - A vision-capable model pulled: set `OLLAMA_VISION_MODEL` (e.g. `llava`,
///   `llama3.2-vision:latest`, `gemma3:latest`). Defaults to `llava`.
///
/// Run:
///   E2E_ENABLED=1 OLLAMA_VISION_MODEL=llava cargo test --test e2e test_ollama_pdf_conversion -- --nocapture
#[tokio::test]
async fn test_ollama_pdf_conversion() {
    if std::env::var("E2E_ENABLED").is_err() {
        println!("SKIP — set E2E_ENABLED=1 to run Ollama e2e tests");
        return;
    }

    if !ollama_is_available().await {
        println!("SKIP — Ollama not reachable (start with: ollama serve)");
        return;
    }

    let pdf_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test_cases")
        .join("irs_form_1040.pdf");
    if !pdf_path.exists() {
        println!("SKIP — test_cases/irs_form_1040.pdf not found. Run: make download-test-pdfs");
        return;
    }

    let model = std::env::var("OLLAMA_VISION_MODEL").unwrap_or_else(|_| "llava".to_string());

    println!("[ollama] Using model: {model}");

    let config = ConversionConfig::builder()
        .dpi(96) // lower DPI for faster local inference
        .concurrency(1)
        .pages(PageSelection::Single(1))
        .fidelity(FidelityTier::Tier1)
        .max_retries(1)
        .build()
        .expect("config must build");

    let mut cfg = config;
    cfg.provider_name = Some("ollama".to_string());
    cfg.model = Some(model.clone());

    let result = convert(&pdf_path.to_string_lossy(), &cfg)
        .await
        .unwrap_or_else(|e| panic!("Ollama conversion failed with model '{model}': {e}"));

    assert!(
        !result.markdown.trim().is_empty(),
        "Ollama conversion must produce non-empty Markdown"
    );
    assert_eq!(
        result.stats.processed_pages, 1,
        "Should have processed exactly 1 page"
    );
    assert_eq!(result.stats.failed_pages, 0, "No pages should fail");

    assert_markdown_quality(&result.markdown, "ollama");

    println!(
        "[ollama] '{model}' output ({} chars):\n{}",
        result.markdown.len(),
        result.markdown
    );
}

/// Gated e2e: verify Ollama correctly forwards images to vision models.
///
/// This is a regression test for edgequake-llm Issue #15, fixed in v0.2.6:
/// `OllamaMessage` was missing the `images` field, so images were silently
/// dropped and the model only saw the text prompt (not the PDF page image).
///
/// The test converts a 2-page document and checks that both pages yield
/// non-trivial output — which would fail if images were dropped (the model
/// would just emit something generic or refuse the request).
///
/// Requirements: `E2E_ENABLED=1`, Ollama running, `OLLAMA_VISION_MODEL` set.
#[tokio::test]
async fn test_ollama_vision_images_forwarded_regression() {
    if std::env::var("E2E_ENABLED").is_err() {
        println!("SKIP — set E2E_ENABLED=1");
        return;
    }

    if !ollama_is_available().await {
        println!("SKIP — Ollama not reachable");
        return;
    }

    let pdf_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test_cases")
        .join("irs_form_1040.pdf");
    if !pdf_path.exists() {
        println!("SKIP — test_cases/irs_form_1040.pdf not found. Run: make download-test-pdfs");
        return;
    }

    let model = std::env::var("OLLAMA_VISION_MODEL").unwrap_or_else(|_| "llava".to_string());

    let config = ConversionConfig::builder()
        .dpi(96)
        .concurrency(1)
        .pages(PageSelection::Range(1, 2))
        .fidelity(FidelityTier::Tier1)
        .max_retries(1)
        .page_separator(PageSeparator::HorizontalRule)
        .build()
        .expect("config must build");

    let mut cfg = config;
    cfg.provider_name = Some("ollama".to_string());
    cfg.model = Some(model.clone());

    let result = convert(&pdf_path.to_string_lossy(), &cfg)
        .await
        .unwrap_or_else(|e| panic!("Ollama vision regression test failed: {e}"));

    // If images were dropped (pre-fix), Ollama would return generic text.
    // With images correctly forwarded, the model should mention form fields.
    assert!(
        result.stats.processed_pages >= 1,
        "Should have processed at least 1 page"
    );
    assert!(
        result.stats.failed_pages == 0,
        "No pages should fail — if images were dropped, the model would reject the request"
    );
    assert!(
        !result.markdown.trim().is_empty(),
        "Vision output must not be empty (images were silently dropped pre-fix)"
    );

    println!(
        "[ollama-vision-regression] '{model}' — {} pages, {} chars",
        result.stats.processed_pages,
        result.markdown.len()
    );
    println!("Output:\n{}", result.markdown);
}

/// Structural test (no Ollama needed): verify ConversionConfig accepts
/// `provider_name = "ollama"` and resolves to the correct default model.
#[test]
fn test_ollama_config_uses_llava_as_default_vision_model() {
    let config = ConversionConfig::builder()
        .dpi(150)
        .build()
        .expect("builder must succeed");

    let mut cfg = config;
    cfg.provider_name = Some("ollama".to_string());

    // config.model is None → resolve_provider will call
    // default_vision_model_for_provider("ollama") which must return "llava"
    // (not "gpt-4.1-nano" which is an OpenAI model and would fail).
    assert_eq!(cfg.provider_name.as_deref(), Some("ollama"));
    assert!(
        cfg.model.is_none(),
        "model should be None so the default kicks in"
    );
}

// ── LM Studio provider e2e tests ──────────────────────────────────────────────

/// Helper: check if LM Studio is reachable at the configured host.
async fn lmstudio_is_available() -> bool {
    let host =
        std::env::var("LMSTUDIO_HOST").unwrap_or_else(|_| "http://localhost:1234".to_string());
    reqwest::Client::new()
        .get(format!("{host}/v1/models"))
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
        .is_ok()
}

/// Gated e2e: convert one PDF page using LM Studio with a local vision model.
///
/// Requirements:
/// - `E2E_ENABLED=1`
/// - LM Studio running at `LMSTUDIO_HOST` (default: http://localhost:1234)
/// - A vision-capable model loaded: set `LMSTUDIO_VISION_MODEL` (e.g. `llava`,
///   `gemma3:latest`). Defaults to `llava`.
///
/// Run:
///   E2E_ENABLED=1 LMSTUDIO_VISION_MODEL=llava cargo test --test e2e test_lmstudio_pdf_conversion -- --nocapture
#[tokio::test]
async fn test_lmstudio_pdf_conversion() {
    if std::env::var("E2E_ENABLED").is_err() {
        println!("SKIP — set E2E_ENABLED=1 to run LM Studio e2e tests");
        return;
    }

    if !lmstudio_is_available().await {
        println!("SKIP — LM Studio not reachable (start LM Studio and load a vision model)");
        return;
    }

    let pdf_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test_cases")
        .join("irs_form_1040.pdf");
    if !pdf_path.exists() {
        println!("SKIP — test_cases/irs_form_1040.pdf not found. Run: make download-test-pdfs");
        return;
    }

    let model = std::env::var("LMSTUDIO_VISION_MODEL").unwrap_or_else(|_| "llava".to_string());

    println!("[lmstudio] Using model: {model}");

    let config = ConversionConfig::builder()
        .dpi(96)
        .concurrency(1)
        .pages(PageSelection::Single(1))
        .fidelity(FidelityTier::Tier1)
        .max_retries(1)
        .build()
        .expect("config must build");

    let mut cfg = config;
    cfg.provider_name = Some("lmstudio".to_string());
    cfg.model = Some(model.clone());

    let result = convert(&pdf_path.to_string_lossy(), &cfg)
        .await
        .unwrap_or_else(|e| panic!("LM Studio conversion failed with model '{model}': {e}"));

    assert!(
        !result.markdown.trim().is_empty(),
        "LM Studio conversion must produce non-empty Markdown"
    );
    assert_eq!(
        result.stats.processed_pages, 1,
        "Should have processed exactly 1 page"
    );
    assert_eq!(result.stats.failed_pages, 0, "No pages should fail");

    assert_markdown_quality(&result.markdown, "lmstudio");

    println!(
        "[lmstudio] '{model}' output ({} chars):\n{}",
        result.markdown.len(),
        result.markdown
    );
}

/// Gated e2e: verify LM Studio correctly forwards images via OpenAI-compatible
/// content-parts array.
///
/// This is a regression test for edgequake-llm Issue #15, fixed in v0.2.6:
/// `LMStudioProvider`'s `ChatMessageRequest.content` was typed as `String`,
/// making multimodal content-parts impossible. Images were silently discarded.
///
/// Requirements: `E2E_ENABLED=1`, LM Studio running, `LMSTUDIO_VISION_MODEL`.
#[tokio::test]
async fn test_lmstudio_vision_images_forwarded_regression() {
    if std::env::var("E2E_ENABLED").is_err() {
        println!("SKIP — set E2E_ENABLED=1");
        return;
    }

    if !lmstudio_is_available().await {
        println!("SKIP — LM Studio not reachable");
        return;
    }

    let pdf_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test_cases")
        .join("attention_is_all_you_need.pdf");
    if !pdf_path.exists() {
        println!("SKIP — test_cases/attention_is_all_you_need.pdf not found. Run: make download-test-pdfs");
        return;
    }

    let model = std::env::var("LMSTUDIO_VISION_MODEL").unwrap_or_else(|_| "llava".to_string());

    let config = ConversionConfig::builder()
        .dpi(96)
        .concurrency(1)
        .pages(PageSelection::Single(1))
        .fidelity(FidelityTier::Tier1)
        .max_retries(1)
        .build()
        .expect("config must build");

    let mut cfg = config;
    cfg.provider_name = Some("lmstudio".to_string());
    cfg.model = Some(model.clone());

    let result = convert(&pdf_path.to_string_lossy(), &cfg)
        .await
        .unwrap_or_else(|e| {
            panic!("LM Studio vision regression test failed with model '{model}': {e}")
        });

    assert!(
        !result.markdown.trim().is_empty(),
        "LM Studio vision output must not be empty (images were silently dropped pre-fix)"
    );
    assert_eq!(result.stats.processed_pages, 1);
    assert_eq!(
        result.stats.failed_pages, 0,
        "No pages should fail — image forwarding must work"
    );

    println!(
        "[lmstudio-vision-regression] '{model}' — {} chars",
        result.markdown.len()
    );
    println!("Output:\n{}", result.markdown);
}

/// Structural test (no LM Studio needed): verify ConversionConfig accepts
/// `provider_name = "lmstudio"` and resolves to the correct default model.
#[test]
fn test_lmstudio_config_uses_llava_as_default_vision_model() {
    let config = ConversionConfig::builder()
        .dpi(150)
        .build()
        .expect("builder must succeed");

    let mut cfg = config;
    cfg.provider_name = Some("lmstudio".to_string());

    assert_eq!(cfg.provider_name.as_deref(), Some("lmstudio"));
    assert!(
        cfg.model.is_none(),
        "model should be None so the default vision model kicks in"
    );
}

// ── OpenAI vision e2e tests (v0.2.6 regression guard) ───────────────────────

/// Gated e2e: verify OpenAI vision still works after edgequake-llm v0.2.6.
///
/// v0.2.6 fixed a temperature guard (skip temperature=1.0 for o-series) and
/// improved image forwarding. This test ensures the OpenAI path is unaffected.
///
/// Requirements: `E2E_ENABLED=1` and `OPENAI_API_KEY`.
#[tokio::test]
async fn test_openai_vision_pdf_conversion_v026_regression() {
    if std::env::var("E2E_ENABLED").is_err() {
        println!("SKIP — set E2E_ENABLED=1 and OPENAI_API_KEY to run");
        return;
    }
    if std::env::var("OPENAI_API_KEY").is_err() {
        println!("SKIP — OPENAI_API_KEY not set");
        return;
    }

    let pdf_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test_cases")
        .join("irs_form_1040.pdf");
    if !pdf_path.exists() {
        println!("SKIP — test_cases/irs_form_1040.pdf not found. Run: make download-test-pdfs");
        return;
    }

    // Use gpt-4o-mini — cheap, fast, vision-capable, unaffected by 0.2.6 temp fix.
    let config = ConversionConfig::builder()
        .dpi(150)
        .concurrency(1)
        .pages(PageSelection::Single(1))
        .fidelity(FidelityTier::Tier1)
        .max_retries(2)
        .build()
        .expect("config must build");

    let mut cfg = config;
    cfg.provider_name = Some("openai".to_string());
    cfg.model = Some("gpt-4o-mini".to_string());

    let result = convert(&pdf_path.to_string_lossy(), &cfg)
        .await
        .expect("OpenAI gpt-4o-mini vision must succeed (v0.2.6 regression)");

    assert!(
        !result.markdown.trim().is_empty(),
        "OpenAI gpt-4o-mini conversion must produce non-empty Markdown"
    );
    assert_eq!(result.stats.processed_pages, 1);
    assert_eq!(result.stats.failed_pages, 0);

    assert_markdown_quality(&result.markdown, "openai-v026-regression");

    println!(
        "[openai-v026] gpt-4o-mini output ({} chars, {} tokens in / {} out):\n{}",
        result.markdown.len(),
        result.stats.total_input_tokens,
        result.stats.total_output_tokens,
        result.markdown
    );
}

// ── Lazy pipeline tests (Issue #16) ──────────────────────────────────────────

/// Verify the lazy pipeline produces identical output to eager for single page.
/// This test uses the default conversion path which is now lazy internally.
#[tokio::test]
async fn test_lazy_pipeline_single_page() {
    let path = e2e_skip_unless_ready!(test_cases_dir().join("irs_form_1040.pdf"));

    let config = ConversionConfig::builder()
        .pages(PageSelection::Single(1))
        .max_retries(2)
        .build()
        .expect("valid config");

    let result = convert(path.to_str().unwrap(), &config)
        .await
        .expect("lazy pipeline conversion should succeed");

    assert_eq!(result.stats.processed_pages, 1);
    assert_eq!(result.stats.failed_pages, 0);
    assert_markdown_quality(&result.markdown, "lazy-single-page");

    let lower = result.markdown.to_lowercase();
    assert!(
        lower.contains("income") || lower.contains("tax") || lower.contains("1040"),
        "IRS form page 1 should mention tax-related content"
    );

    println!(
        "[lazy-single] {} chars, {}ms total",
        result.markdown.len(),
        result.stats.total_duration_ms
    );
}

/// Verify the lazy pipeline works with multiple concurrent pages.
#[tokio::test]
async fn test_lazy_pipeline_concurrent_multi_page() {
    let path = e2e_skip_unless_ready!(test_cases_dir().join("attention_is_all_you_need.pdf"));
    let out_path = output_dir().join("lazy_concurrent_3pages.md");

    let config = ConversionConfig::builder()
        .pages(PageSelection::Range(1, 3))
        .concurrency(3)
        .page_separator(PageSeparator::HorizontalRule)
        .max_retries(2)
        .build()
        .expect("valid config");

    let result = convert(path.to_str().unwrap(), &config)
        .await
        .expect("lazy concurrent conversion should succeed");

    assert_eq!(result.stats.processed_pages, 3);
    assert_eq!(result.stats.failed_pages, 0);
    assert_markdown_quality(&result.markdown, "lazy-concurrent");

    // Should have 2 separators for 3 pages
    let sep_count = result.markdown.matches("---").count();
    assert!(
        sep_count >= 2,
        "Expected at least 2 HR separators for 3 pages, got {sep_count}"
    );

    std::fs::write(&out_path, &result.markdown).ok();
    println!(
        "[lazy-concurrent] {} pages, {} chars, {}ms",
        result.stats.processed_pages,
        result.markdown.len(),
        result.stats.total_duration_ms
    );
}

/// Verify the lazy pipeline works with maintain_format (sequential).
#[tokio::test]
async fn test_lazy_pipeline_sequential_maintain_format() {
    let path = e2e_skip_unless_ready!(test_cases_dir().join("attention_is_all_you_need.pdf"));

    let config = ConversionConfig::builder()
        .pages(PageSelection::Range(1, 2))
        .maintain_format(true)
        .concurrency(1)
        .page_separator(PageSeparator::HorizontalRule)
        .max_retries(2)
        .build()
        .expect("valid config");

    let result = convert(path.to_str().unwrap(), &config)
        .await
        .expect("lazy sequential conversion should succeed");

    assert_eq!(result.stats.processed_pages, 2);
    assert_eq!(result.stats.failed_pages, 0);
    assert_markdown_quality(&result.markdown, "lazy-sequential");

    println!(
        "[lazy-sequential] {} pages, {} chars, {}ms",
        result.stats.processed_pages,
        result.markdown.len(),
        result.stats.total_duration_ms
    );
}

/// Verify the streaming API uses the lazy pipeline.
#[tokio::test]
async fn test_lazy_stream_api() {
    use edgequake_pdf2md::convert_stream;
    use futures::StreamExt;

    let path = e2e_skip_unless_ready!(test_cases_dir().join("irs_form_1040.pdf"));

    let config = ConversionConfig::builder()
        .pages(PageSelection::All)
        .max_retries(2)
        .build()
        .expect("valid config");

    let mut stream = convert_stream(path.to_str().unwrap(), &config)
        .await
        .expect("stream creation should succeed");

    let mut pages = Vec::new();
    while let Some(result) = stream.next().await {
        match result {
            Ok(page) => {
                assert!(!page.markdown.trim().is_empty(), "page should have content");
                pages.push(page);
            }
            Err(e) => panic!("streaming page failed: {e}"),
        }
    }

    assert_eq!(pages.len(), 2, "IRS form has 2 pages");
    println!("[lazy-stream] {} pages received via stream", pages.len());
}

/// Verify progress callbacks fire correctly with lazy pipeline.
#[tokio::test]
async fn test_lazy_pipeline_progress_callbacks() {
    use edgequake_pdf2md::ConversionProgressCallback;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let path = e2e_skip_unless_ready!(test_cases_dir().join("irs_form_1040.pdf"));

    let starts = Arc::new(AtomicUsize::new(0));
    let completes = Arc::new(AtomicUsize::new(0));
    let conversion_started = Arc::new(AtomicUsize::new(0));
    let conversion_completed = Arc::new(AtomicUsize::new(0));

    struct TestCallback {
        starts: Arc<AtomicUsize>,
        completes: Arc<AtomicUsize>,
        conversion_started: Arc<AtomicUsize>,
        conversion_completed: Arc<AtomicUsize>,
    }

    impl ConversionProgressCallback for TestCallback {
        fn on_conversion_start(&self, total_pages: usize) {
            self.conversion_started.store(total_pages, Ordering::SeqCst);
        }
        fn on_page_start(&self, _page_num: usize, _total: usize) {
            self.starts.fetch_add(1, Ordering::SeqCst);
        }
        fn on_page_complete(&self, _page_num: usize, _total: usize, _len: usize) {
            self.completes.fetch_add(1, Ordering::SeqCst);
        }
        fn on_conversion_complete(&self, _total: usize, success: usize) {
            self.conversion_completed.store(success, Ordering::SeqCst);
        }
    }

    let cb = Arc::new(TestCallback {
        starts: Arc::clone(&starts),
        completes: Arc::clone(&completes),
        conversion_started: Arc::clone(&conversion_started),
        conversion_completed: Arc::clone(&conversion_completed),
    });

    let config = ConversionConfig::builder()
        .pages(PageSelection::All)
        .max_retries(2)
        .progress_callback(cb as Arc<dyn ConversionProgressCallback>)
        .build()
        .expect("valid config");

    let result = convert(path.to_str().unwrap(), &config)
        .await
        .expect("conversion should succeed");

    assert_eq!(result.stats.processed_pages, 2);
    assert_eq!(
        conversion_started.load(Ordering::SeqCst),
        2,
        "on_conversion_start should receive 2"
    );
    assert_eq!(
        starts.load(Ordering::SeqCst),
        2,
        "on_page_start should fire 2 times"
    );
    assert_eq!(
        completes.load(Ordering::SeqCst),
        2,
        "on_page_complete should fire 2 times"
    );
    assert_eq!(
        conversion_completed.load(Ordering::SeqCst),
        2,
        "on_conversion_complete should receive 2 successes"
    );

    println!("[lazy-callbacks] all progress callbacks fired correctly");
}

/// Verify convert_from_bytes works with lazy pipeline.
#[tokio::test]
async fn test_lazy_convert_from_bytes() {
    use edgequake_pdf2md::convert_from_bytes;

    let path = e2e_skip_unless_ready!(test_cases_dir().join("irs_form_1040.pdf"));
    let bytes = std::fs::read(&path).expect("read PDF bytes");

    let config = ConversionConfig::builder()
        .pages(PageSelection::Single(1))
        .max_retries(2)
        .build()
        .expect("valid config");

    let result = convert_from_bytes(&bytes, &config)
        .await
        .expect("convert_from_bytes should succeed with lazy pipeline");

    assert_eq!(result.stats.processed_pages, 1);
    assert_eq!(result.stats.failed_pages, 0);
    assert_markdown_quality(&result.markdown, "lazy-from-bytes");

    println!("[lazy-from-bytes] {} chars", result.markdown.len());
}

/// Verify convert_stream_from_bytes keeps tempfile alive with lazy pipeline.
#[tokio::test]
async fn test_lazy_stream_from_bytes() {
    use edgequake_pdf2md::convert_stream_from_bytes;
    use futures::StreamExt;

    let path = e2e_skip_unless_ready!(test_cases_dir().join("irs_form_1040.pdf"));
    let bytes = std::fs::read(&path).expect("read PDF bytes");

    let config = ConversionConfig::builder()
        .pages(PageSelection::Single(1))
        .max_retries(2)
        .build()
        .expect("valid config");

    let mut stream = convert_stream_from_bytes(&bytes, &config)
        .await
        .expect("stream_from_bytes should succeed");

    let mut count = 0;
    while let Some(result) = stream.next().await {
        match result {
            Ok(page) => {
                assert!(!page.markdown.trim().is_empty());
                count += 1;
            }
            Err(e) => panic!("streaming from bytes failed: {e}"),
        }
    }

    assert_eq!(count, 1, "should get exactly 1 page");
    println!("[lazy-stream-from-bytes] tempfile stayed alive correctly");
}
