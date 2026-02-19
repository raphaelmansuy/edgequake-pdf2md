# 08 — Error Handling

> **See also**: [Index](./00-index.md) · [API Design](./06-api-design.md) · [Algorithm](./02-algorithm.md) · [CLI Design](./07-cli-design.md)

---

## 1. Design Principles

1. **Library returns typed errors** — `Pdf2MdError` with all variants in one `thiserror` enum
2. **CLI uses `anyhow`** — wraps library errors with human-readable context
3. **Page errors are non-fatal** — a failed page produces a `PageError` in the output, not an abort
4. **Only true failures abort** — file not found, corrupt PDF, missing API key
5. **Errors are actionable** — every error message says what to do next
6. **No panics in library code** — only `unwrap()` on programming invariants, guarded by debug assertions

**External references**:
- [thiserror v2](https://docs.rs/thiserror/latest/thiserror/)
- [anyhow v2](https://docs.rs/anyhow/latest/anyhow/)

---

## 2. Error Type Hierarchy

```
Pdf2MdError                          (library crate, thiserror)
├── InputError
│   ├── FileNotFound
│   ├── PermissionDenied
│   ├── InvalidUrl
│   └── NotAPdf
├── PdfError
│   ├── CorruptPdf
│   ├── PasswordRequired
│   ├── WrongPassword
│   ├── PageOutOfRange
│   └── RasterisationFailed
├── NetworkError
│   ├── DownloadFailed
│   └── Timeout
├── LlmError
│   ├── ApiError
│   ├── RateLimited
│   ├── ContextTooLong
│   └── AllPagesFailed
├── IoError
│   └── OutputWriteFailed
└── ConfigError
    ├── InvalidDpi
    ├── InvalidConcurrency
    └── ProviderNotConfigured

PageError                             (non-fatal, embedded in ConversionOutput)
├── RenderFailed(usize, String)
├── LlmFailed(usize, String)
└── Timeout(usize)
```

---

## 3. Pdf2MdError Enum

```rust
use thiserror::Error;

/// All fatal errors returned by the edgequake-pdf2md library.
///
/// Page-level failures use [`PageError`] and are stored in
/// [`ConversionOutput::pages`] rather than propagated here.
#[derive(Debug, Error)]
pub enum Pdf2MdError {
    // ── Input errors ──────────────────────────────────────────────────────

    /// Input file was not found at the given path.
    #[error("PDF file not found: '{path}'\nCheck the path exists and is readable.")]
    FileNotFound { path: PathBuf },

    /// Process does not have read permission on the file.
    #[error("Permission denied reading '{path}'\nTry: chmod +r {path:?}")]
    PermissionDenied { path: PathBuf },

    /// The input string is not a valid file path or URL.
    #[error("Invalid input '{input}': not a file path or a valid HTTP/HTTPS URL")]
    InvalidInput { input: String },

    /// HTTP URL was syntactically valid but download failed.
    #[error("Failed to download '{url}': {reason}\nCheck your internet connection.")]
    DownloadFailed { url: String, reason: String },

    /// Download exceeded the configured timeout.
    #[error("Download timed out after {secs}s for '{url}'\nIncrease --download-timeout.")]
    DownloadTimeout { url: String, secs: u64 },

    /// The file exists and was read, but is not a PDF.
    #[error("File is not a valid PDF: '{path}'\nFirst bytes: {magic:?}")]
    NotAPdf { path: PathBuf, magic: [u8; 4] },

    // ── PDF errors ────────────────────────────────────────────────────────

    /// PDF header/trailer/xref is corrupt and cannot be parsed.
    #[error("PDF '{path}' is corrupt: {detail}\nTry repairing with: qpdf --decrypt input.pdf output.pdf")]
    CorruptPdf { path: PathBuf, detail: String },

    /// PDF requires a password but none was provided.
    #[error("PDF '{path}' is encrypted and requires a password.\nProvide it with --password <PASSWORD>.")]
    PasswordRequired { path: PathBuf },

    /// A password was provided but it is wrong.
    #[error("Wrong password for PDF '{path}'")]
    WrongPassword { path: PathBuf },

    /// Selected page numbers exceed the actual page count.
    #[error("Page {page} is out of range (document has {total} pages)")]
    PageOutOfRange { page: usize, total: usize },

    /// pdfium-render returned an error for a specific page.
    #[error("Rasterisation failed for page {page}: {detail}")]
    RasterisationFailed { page: usize, detail: String },

    // ── LLM errors ────────────────────────────────────────────────────────

    /// The configured provider is not initialised (missing API key etc.).
    #[error("LLM provider '{provider}' is not configured.\n{hint}")]
    ProviderNotConfigured { provider: String, hint: String },

    /// The LLM API returned a non-retryable error.
    #[error("LLM API error: {message}")]
    LlmApiError { message: String },

    /// Every page failed after all retries; output would be empty.
    #[error("All {total} pages failed after {retries} retries each.\nFirst error: {first_error}")]
    AllPagesFailed { total: usize, retries: u32, first_error: String },

    // ── I/O errors ────────────────────────────────────────────────────────

    /// Could not create or write the output Markdown file.
    #[error("Failed to write output file '{path}': {source}")]
    OutputWriteFailed {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    // ── Config errors ─────────────────────────────────────────────────────

    /// Builder validation failed.
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    // ── Catch-all ─────────────────────────────────────────────────────────

    /// Unexpected internal error (should never appear in production).
    #[error("Internal error: {0}")]
    Internal(String),
}
```

---

## 4. PageError (Non-fatal)

```rust
/// A non-fatal error for a single page.
///
/// Stored in [`PageResult`] when a page fails. The overall conversion
/// continues unless ALL pages fail (which promotes to [`Pdf2MdError::AllPagesFailed`]).
#[derive(Debug, Clone, Error, Serialize, Deserialize)]
pub enum PageError {
    #[error("Page {page}: rasterisation failed: {detail}")]
    RenderFailed { page: usize, detail: String },

    #[error("Page {page}: LLM call failed after {retries} retries: {detail}")]
    LlmFailed { page: usize, retries: u8, detail: String },

    #[error("Page {page}: LLM call timed out after {secs}s")]
    Timeout { page: usize, secs: u64 },
}
```

`PageResult` carries an `Option<PageError>`. When `Some`, the `markdown` field is an empty string.

---

## 5. Recovery Strategies

| Scenario | Strategy | Outcome |
|----------|----------|---------|
| Page render fails | Skip page, record `PageError::RenderFailed` | Partial output |
| LLM rate limited (429) | Exponential backoff, retry up to `max_retries` | Transparent to caller |
| LLM 5xx error | Exponential backoff, retry up to `max_retries` | Transparent to caller |
| LLM context overflow | Reduce DPI, re-encode at lower size, retry once | Transparent to caller |
| LLM non-retryable (4xx) | Record `PageError::LlmFailed`, continue | Partial output |
| LLM timeout | Record `PageError::Timeout`, continue | Partial output |
| ALL pages failed | Return `Pdf2MdError::AllPagesFailed` | Fatal |
| File not found | Return `Pdf2MdError::FileNotFound` immediately | Fatal |
| Corrupt PDF | Return `Pdf2MdError::CorruptPdf` immediately | Fatal |
| Download fails | Return `Pdf2MdError::DownloadFailed` immediately | Fatal |

---

## 6. Retry Logic

```
Attempt 1 fails (e.g. 429 rate limited)
  → wait retry_backoff_ms * 2^0 = 500ms
Attempt 2 fails
  → wait retry_backoff_ms * 2^1 = 1000ms
Attempt 3 fails
  → wait retry_backoff_ms * 2^2 = 2000ms
Attempt 4 = max_retries + 1: give up, record PageError
```

Maximum jitter of ±10% is added to each backoff to prevent thundering herd.
Hard ceiling on backoff: 30 seconds.

```rust
fn backoff_ms(attempt: u32, base_ms: u64) -> u64 {
    let exp = base_ms.saturating_mul(1 << attempt.min(6));  // cap at 2^6
    let jitter = (exp as f64 * 0.1 * (rand::random::<f64>() - 0.5)) as i64;
    (exp as i64 + jitter).clamp(0, 30_000) as u64
}
```

---

## 7. Provider-Not-Configured Hints

`ProviderNotConfigured` includes a human-readable `hint` field:

| Provider | Hint |
|----------|------|
| openai | `Set OPENAI_API_KEY=sk-...` |
| anthropic | `Set ANTHROPIC_API_KEY=sk-ant-...` |
| azure | `Set AZURE_OPENAI_API_KEY, AZURE_OPENAI_ENDPOINT, AZURE_OPENAI_DEPLOYMENT` |
| gemini | `Set GEMINI_API_KEY=...` |
| ollama | `Start Ollama: ollama serve, then: ollama pull llava` |
| lmstudio | `Start LM Studio and load a vision model, default endpoint: http://localhost:1234` |
| openrouter | `Set OPENROUTER_API_KEY=sk-or-...` |
| xai | `Set XAI_API_KEY=...` |
| huggingface | `Set HUGGINGFACE_API_KEY=hf-...` |

---

## 8. CLI Error Output

The CLI binary uses `anyhow` to wrap library errors with execution context:

```rust
// bin/pdf2md.rs
let result = convert(&cli.input, &config).await
    .with_context(|| format!("Converting '{}'", cli.input))?;
```

Error output goes to **stderr**. Format:

```
Error: Converting 'document.pdf'

Caused by:
    PDF 'document.pdf' is encrypted and requires a password.
    Provide it with --password <PASSWORD>.
```

The CLI exits with code `1` for `Pdf2MdError` and code `3` for `ConfigError`.

---

## 9. Tracing Integration

Every error path emits a `tracing` event before returning:

```rust
// Fatal
tracing::error!(
    error = %e,
    path = %path.display(),
    "pdf rasterisation failed"
);

// Per-page recoverable
tracing::warn!(
    page = page_num,
    attempt = attempt,
    error = %api_error,
    "llm call failed, retrying"
);
```

Users can enable structured JSON logs with:
```
RUST_LOG=edgequake_pdf2md=debug pdf2md report.pdf 2>debug.log
```

---

## 10. Test Checklist

See [Testing Strategy](./09-testing-strategy.md) for full detail. Error-specific tests:

- [ ] `FileNotFound` returned when path does not exist
- [ ] `NotAPdf` returned for a text file renamed `.pdf`
- [ ] `PasswordRequired` returned for encrypted PDF without password
- [ ] `WrongPassword` returned for encrypted PDF with wrong password
- [ ] `PageOutOfRange` returned when `--pages 999` on a 50-page doc
- [ ] `AllPagesFailed` returned when `MockProvider` always errors
- [ ] `DownloadFailed` returned for 404 URL
- [ ] `OutputWriteFailed` returned when output dir does not exist
- [ ] Partial success (50 pages, page 7 fails) → `stats.failed_pages == 1`
- [ ] Retry succeeds on second attempt → `retries == 1` in `PageResult`
