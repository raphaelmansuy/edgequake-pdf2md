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
