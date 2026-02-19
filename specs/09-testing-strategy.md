# 09 — Testing Strategy

> **See also**: [Index](./00-index.md) · [Algorithm](./02-algorithm.md) · [API Design](./06-api-design.md) · [Error Handling](./08-error-handling.md)

---

## 1. Testing Pyramid

```
                  ┌────────────┐
                  │  E2E / CLI │  (few, real provider, optional)
                 ┌┴────────────┴┐
                 │  Integration  │  (mocked LLM, real PDF)
                ┌┴───────────────┴┐
                │   Unit / Props   │  (no I/O, fast, exhaustive)
                └──────────────────┘
```

| Layer | Speed | Isolation | Count |
|-------|-------|-----------|-------|
| Unit | <1s each | full (no I/O) | ~60 |
| Integration | 2–30s each | mocked LLM, real pdfium | ~20 |
| Golden-file | 5–60s each | mocked LLM, reference PDFs | ~10 |
| E2E (opt-in) | minutes | real API | ~5 |

---

## 2. Unit Tests

Located in `src/` as `#[cfg(test)]` modules alongside the code they test.

### 2.1 Post-processing (`pipeline/postprocess.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_redundant_horizontal_rules() {
        let input = "# Heading\n---\n---\n text";
        assert_eq!(clean_markdown(input), "# Heading\n---\n text");
    }

    #[test]
    fn normalises_multiple_blank_lines() {
        let input = "line1\n\n\n\nline2";
        assert_eq!(clean_markdown(input), "line1\n\nline2");
    }

    #[test]
    fn trims_leading_trailing_whitespace_per_line() { ... }

    #[test]
    fn preserves_code_blocks_intact() { ... }

    #[test]
    fn preserves_yaml_frontmatter() { ... }

    #[test]
    fn strips_llm_preamble_phrases() {
        // "Here is the markdown:" → stripped
        let input = "Here is the markdown:\n# Title\nbody";
        assert_eq!(clean_markdown(input), "# Title\nbody");
    }

    #[test]
    fn strips_markdown_fences_wrapping_output() {
        // LLM sometimes wraps in ```markdown ... ```
        let input = "```markdown\n# Title\nbody\n```";
        assert_eq!(clean_markdown(input), "# Title\nbody");
    }
}
```

### 2.2 Page Selection (`config.rs`)

```rust
#[test]
fn parse_all() {
    assert_eq!(parse_page_selection("all"), Ok(PageSelection::All));
}

#[test]
fn parse_single() {
    assert_eq!(parse_page_selection("5"), Ok(PageSelection::Single(5)));
}

#[test]
fn parse_range() {
    assert_eq!(parse_page_selection("3-15"), Ok(PageSelection::Range(3..=15)));
}

#[test]
fn parse_set() {
    assert_eq!(parse_page_selection("1,3,5"), Ok(PageSelection::Set(vec![1,3,5])));
}

#[test]
fn parse_zero_is_err() {
    assert!(parse_page_selection("0").is_err());
}

#[test]
fn parse_inverted_range_is_err() {
    assert!(parse_page_selection("15-3").is_err());
}
```

### 2.3 Page Separator Assembly (`pipeline/postprocess.rs`)

```rust
#[test]
fn no_separator_joins_with_blank_line() {
    let pages = vec!["A", "B", "C"];
    let result = assemble_document(&pages, &PageSeparator::None);
    assert_eq!(result, "A\n\nB\n\nC");
}

#[test]
fn horizontal_rule_separator() {
    let result = assemble_document(&["A", "B"], &PageSeparator::HorizontalRule);
    assert_eq!(result, "A\n\n---\n\nB");
}

#[test]
fn comment_separator_has_page_number() {
    let result = assemble_document(&["A", "B"], &PageSeparator::Comment);
    assert!(result.contains("<!-- page 2 -->"));
}
```

### 2.4 Config Builder Validation

```rust
#[test]
fn dpi_clamped_to_min_72() {
    let c = ConversionConfig::builder().dpi(50).build().unwrap();
    assert_eq!(c.dpi, 72);
}

#[test]
fn dpi_clamped_to_max_400() {
    let c = ConversionConfig::builder().dpi(1000).build().unwrap();
    assert_eq!(c.dpi, 400);
}

#[test]
fn concurrency_min_1() {
    let c = ConversionConfig::builder().concurrency(0).build().unwrap();
    assert_eq!(c.concurrency, 1);
}
```

### 2.5 Backoff Calculation

```rust
#[test]
fn backoff_doubles_each_attempt() {
    let b0 = backoff_ms(0, 500);
    let b1 = backoff_ms(1, 500);
    // Allow for jitter; b1 should be roughly 2× b0
    assert!(b1 > b0);
}

#[test]
fn backoff_capped_at_30s() {
    let b = backoff_ms(20, 500);  // would be 2^20 * 500ms without cap
    assert!(b <= 30_000);
}
```

---

## 3. Integration Tests

Located in `tests/`. Use real pdfium rendering but a `MockProvider` LLM.

### 3.1 MockProvider

```rust
// tests/helpers/mock_llm.rs

use edgequake_llm::{LLMProvider, ChatMessage, CompletionResponse};

/// A deterministic mock LLM that returns preset responses.
pub struct MockProvider {
    pub responses: Vec<String>,     // cycled in order
    pub fail_on_pages: Vec<usize>,  // page indices to simulate failure on
    pub latency_ms: u64,
}

impl MockProvider {
    pub fn always_returns(markdown: &str) -> Self { ... }
    pub fn fails_on(pages: Vec<usize>) -> Self { ... }
}

#[async_trait]
impl LLMProvider for MockProvider {
    async fn chat(&self, messages: &[ChatMessage]) -> Result<CompletionResponse> {
        tokio::time::sleep(Duration::from_millis(self.latency_ms)).await;
        // ... return next response in cycle
    }
    // ...
}
```

### 3.2 Reference PDF Fixtures

Test PDFs are stored in `tests/fixtures/`:

| File | Description | Pages |
|------|-------------|-------|
| `single_page.pdf` | One paragraph of Lorem Ipsum | 1 |
| `multi_page.pdf` | 5 pages of mixed content | 5 |
| `table_heavy.pdf` | 3 pages, each with a complex table | 3 |
| `scanned.pdf` | Image-only (scanned) document | 2 |
| `rotated_pages.pdf` | Pages at 90°, 180°, 270° rotation | 4 |
| `password_protected.pdf` | Password: "test123" | 2 |
| `large.pdf` | 100 pages | 100 |
| `empty.pdf` | Valid PDF, 0 content pages | 0 |

PDFs are generated using `tests/fixtures/generate.py` (requires `fpdf2`) and committed to the repository.

### 3.3 Integration Test Cases

```rust
// tests/integration/basic.rs

#[tokio::test]
async fn single_page_roundtrip() {
    let provider = MockProvider::always_returns("# Hello\nWorld");
    let config = ConversionConfig::builder()
        .provider(Arc::new(provider))
        .build().unwrap();
    let output = convert("tests/fixtures/single_page.pdf", &config).await.unwrap();
    assert_eq!(output.pages.len(), 1);
    assert_eq!(output.stats.failed_pages, 0);
    assert!(output.markdown.contains("# Hello"));
}

#[tokio::test]
async fn multi_page_all_pages_processed() {
    let provider = MockProvider::always_returns("page content");
    let config = ...;
    let output = convert("tests/fixtures/multi_page.pdf", &config).await.unwrap();
    assert_eq!(output.pages.len(), 5);
    assert_eq!(output.stats.processed_pages, 5);
}

#[tokio::test]
async fn password_protected_with_correct_password_succeeds() {
    let config = ConversionConfig::builder()
        .password("test123")
        ...build().unwrap();
    let output = convert("tests/fixtures/password_protected.pdf", &config).await.unwrap();
    assert!(!output.markdown.is_empty());
}

#[tokio::test]
async fn password_protected_without_password_returns_error() {
    let config = ConversionConfig::default();
    let err = convert("tests/fixtures/password_protected.pdf", &config).await.unwrap_err();
    assert!(matches!(err, Pdf2MdError::PasswordRequired { .. }));
}

#[tokio::test]
async fn page_selection_range_processes_only_selected_pages() {
    let provider = MockProvider::always_returns("content");
    let config = ConversionConfig::builder()
        .pages(PageSelection::Range(2..=4))
        .provider(Arc::new(provider))
        .build().unwrap();
    let output = convert("tests/fixtures/multi_page.pdf", &config).await.unwrap();
    assert_eq!(output.pages.len(), 3);
    assert_eq!(output.pages[0].page_num, 2);
    assert_eq!(output.pages[2].page_num, 4);
}

#[tokio::test]
async fn all_pages_failed_returns_error() {
    let provider = MockProvider::fails_on(vec![1,2,3,4,5]);
    let config = ...;
    let err = convert("tests/fixtures/multi_page.pdf", &config).await.unwrap_err();
    assert!(matches!(err, Pdf2MdError::AllPagesFailed { .. }));
}

#[tokio::test]
async fn one_page_failed_returns_partial_output() {
    let provider = MockProvider::fails_on(vec![3]);
    let config = ...;
    let output = convert("tests/fixtures/multi_page.pdf", &config).await.unwrap();
    assert_eq!(output.stats.failed_pages, 1);
    assert_eq!(output.stats.processed_pages, 4);
}

#[tokio::test]
async fn streaming_api_emits_all_pages() {
    let provider = MockProvider::always_returns("content");
    let config = ...;
    let stream = convert_stream("tests/fixtures/multi_page.pdf", &config).await.unwrap();
    let pages: Vec<_> = stream.collect().await;
    assert_eq!(pages.len(), 5);
}

#[tokio::test]
async fn maintain_format_processes_sequentially() {
    // verify each call includes prior page as context
    let tracker = Arc::new(Mutex::new(vec![]));
    let provider = TrackingMockProvider { tracker: tracker.clone(), ... };
    let config = ConversionConfig::builder()
        .maintain_format(true)
        ...build().unwrap();
    convert("tests/fixtures/multi_page.pdf", &config).await.unwrap();
    // second message in each call after page 1 should include prior page markdown
    let calls = tracker.lock().unwrap();
    assert!(calls[1].iter().any(|m| m.role == "assistant"));
}
```

---

## 4. Golden-File Tests

Golden-file tests compare the Markdown output of real models against known-good reference files.

```
tests/
└── golden/
    ├── fixtures/        (input PDFs)
    │   ├── arxiv_sample.pdf
    │   └── invoice_sample.pdf
    └── expected/        (expected markdown output)
        ├── arxiv_sample.gpt-4o.md
        └── invoice_sample.gpt-4o.md
```

### Running golden tests

```
PDF2MD_GOLDEN=1 OPENAI_API_KEY=sk-... cargo test --test golden
```

By default (no `PDF2MD_GOLDEN` env), golden tests are **skipped** to allow offline CI.

### Updating golden baselines

```
PDF2MD_GOLDEN_UPDATE=1 OPENAI_API_KEY=sk-... cargo test --test golden
```

This overwrites `tests/golden/expected/*.md` with current output.

### Regression check

```rust
#[tokio::test]
#[cfg_attr(not(pdf2md_golden), ignore)]
async fn golden_arxiv_sample() {
    let config = ConversionConfig::builder()
        .model("gpt-4o")
        .build().unwrap();
    let output = convert("tests/golden/fixtures/arxiv_sample.pdf", &config).await.unwrap();
    let expected = std::fs::read_to_string("tests/golden/expected/arxiv_sample.gpt-4o.md")
        .expect("golden file missing; run with PDF2MD_GOLDEN_UPDATE=1 to generate");
    assert_similar_markdown(&output.markdown, &expected, 0.90); // 90% ROUGE-L threshold
}
```

---

## 5. Property-Based Tests

Use [`proptest`](https://docs.rs/proptest/latest/proptest/) for input fuzzing:

```toml
[dev-dependencies]
proptest = "1"
```

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn clean_markdown_never_panics(s in ".*") {
        let _ = clean_markdown(&s);
    }

    #[test]
    fn parse_page_selection_never_panics(s in "[0-9a-z,\\-]{0,50}") {
        let _ = parse_page_selection(&s);
    }

    #[test]
    fn assemble_document_never_panics(
        pages in prop::collection::vec(".*", 0..100),
        sep in prop::sample::select(vec![
            PageSeparator::None,
            PageSeparator::HorizontalRule,
            PageSeparator::Comment,
        ])
    ) {
        let refs: Vec<&str> = pages.iter().map(|s| s.as_str()).collect();
        let _ = assemble_document(&refs, &sep);
    }
}
```

---

## 6. CLI Integration Tests

```rust
// tests/cli.rs — uses `assert_cmd` and `predicates` crates

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn cli_help_exits_zero() {
    let mut cmd = Command::cargo_bin("pdf2md").unwrap();
    cmd.arg("--help").assert().success();
}

#[test]
fn cli_missing_input_exits_nonzero() {
    Command::cargo_bin("pdf2md").unwrap()
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn cli_file_not_found_exits_1() {
    Command::cargo_bin("pdf2md").unwrap()
        .env("EDGEQUAKE_PROVIDER", "mock")
        .arg("nonexistent.pdf")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn cli_inspect_prints_metadata() {
    Command::cargo_bin("pdf2md").unwrap()
        .arg("--inspect")
        .arg("tests/fixtures/single_page.pdf")
        .assert()
        .success()
        .stdout(predicate::str::contains("Pages:"));
}
```

Additional dev-dependencies:

```toml
[dev-dependencies]
assert_cmd = "2"
predicates = "3"
```

---

## 7. Test Configuration (CI/CD)

### `.cargo/config.toml`

```toml
[env]
# Prevent golden tests from running accidentally in CI
# CI should set PDF2MD_GOLDEN=1 explicitly for the golden test job
PDF2MD_GOLDEN = { value = "0", force = false }
```

### GitHub Actions workflow sketch

```yaml
name: CI

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Download pdfium
        run: |
          curl -L https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-linux-x64.tgz | tar xz
          echo "PDFIUM_DYNAMIC_LIB_PATH=$PWD/lib" >> $GITHUB_ENV
      - run: cargo test --workspace
      - run: cargo clippy --workspace -- -D warnings
      - run: cargo fmt --check

  golden:
    runs-on: ubuntu-latest
    if: github.event_name == 'push' && github.ref == 'refs/heads/main'
    env:
      PDF2MD_GOLDEN: "1"
      OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --test golden
```

---

## 8. Test Coverage Target

| Module | Target Coverage |
|--------|----------------|
| `pipeline/postprocess.rs` | ≥ 95% |
| `pipeline/encode.rs` | ≥ 90% |
| `config.rs` | ≥ 90% |
| `error.rs` | ≥ 85% |
| `convert.rs` | ≥ 80% |
| `pipeline/render.rs` | ≥ 70% (platform-dependent) |
| Overall | ≥ 80% |

Run coverage via:
```
cargo llvm-cov --workspace --html
```

Requires: `cargo install cargo-llvm-cov`.

---

## 9. Quick Reference: Running Tests

```bash
# All unit + integration tests (fast, no API key needed)
cargo test

# Specific module
cargo test pipeline::postprocess

# Integration tests only
cargo test --test integration

# CLI tests
cargo test --test cli

# Golden (requires OPENAI_API_KEY)
PDF2MD_GOLDEN=1 cargo test --test golden

# Update golden baselines
PDF2MD_GOLDEN_UPDATE=1 cargo test --test golden

# Coverage report
cargo llvm-cov --workspace --html && open target/llvm-cov/html/index.html

# Fuzz / proptest
cargo test -- --include-ignored proptest
```
