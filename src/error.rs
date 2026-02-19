//! Error types for the edgequake-pdf2md library.
//!
//! Two distinct error types reflect two distinct failure modes:
//!
//! * [`Pdf2MdError`] — **Fatal**: the conversion cannot proceed at all
//!   (bad input file, wrong password, provider not configured). Returned as
//!   `Err(Pdf2MdError)` from the top-level `convert*` functions.
//!
//! * [`PageError`] — **Non-fatal**: a single page failed (render glitch,
//!   transient API error) but all other pages are fine. Stored inside
//!   [`crate::output::PageResult`] so callers can inspect partial
//!   success rather than losing the whole document to one bad page.
//!
//! The separation lets callers decide their own tolerance: abort on the first
//! page failure, log and continue, or collect all errors for a post-run report.

use std::path::PathBuf;
use thiserror::Error;

/// All fatal errors returned by the edgequake-pdf2md library.
///
/// Page-level failures use [`PageError`] and are stored in
/// [`crate::output::PageResult`] rather than propagated here.
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
    AllPagesFailed {
        total: usize,
        retries: u32,
        first_error: String,
    },

    /// Some pages succeeded but at least one failed.
    ///
    /// Returned by [`crate::output::ConversionOutput::into_result`] when
    /// the caller wants to treat any page failure as an error.
    #[error("{failed}/{total} pages failed during conversion")]
    PartialFailure {
        success: usize,
        failed: usize,
        total: usize,
    },

    /// VLM API returned HTTP 429 — caller should back off.
    ///
    /// Check `retry_after_secs` for a server-specified delay, or use
    /// exponential backoff if `None`.
    #[error("Rate limit exceeded for provider '{provider}'")]
    RateLimitExceeded {
        provider: String,
        retry_after_secs: Option<u64>,
    },

    /// VLM API call timed out — the caller may retry.
    #[error("API call timed out after {elapsed_ms}ms on page {page}")]
    ApiTimeout { page: usize, elapsed_ms: u64 },

    /// VLM API returned an authentication error (401/403) — retry unlikely to help.
    #[error("Authentication error from provider '{provider}': {detail}")]
    AuthError { provider: String, detail: String },

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

    // ── Pdfium binding errors ─────────────────────────────────────────────
    /// Could not bind to a pdfium library.
    #[error(
        "Failed to bind to pdfium library: {0}\n\n\
PDFium is normally downloaded automatically on first run.\n\
If the auto-download failed, you can:\n\
  • Check your internet connection and try again.\n\
  • Set PDFIUM_LIB_PATH=/path/to/libpdfium to use an existing copy.\n\
  • Run `./scripts/setup-pdfium.sh` and set PDFIUM_LIB_PATH to the result.\n"
    )]
    PdfiumBindingFailed(String),

    // ── Catch-all ─────────────────────────────────────────────────────────
    /// Unexpected internal error.
    #[error("Internal error: {0}")]
    Internal(String),
}

/// A non-fatal error for a single page.
///
/// Stored alongside [`crate::output::PageResult`] when a page fails.
/// The overall conversion continues unless ALL pages fail.
#[derive(Debug, Clone, Error, serde::Serialize, serde::Deserialize)]
pub enum PageError {
    /// Page rasterisation failed.
    #[error("Page {page}: rasterisation failed: {detail}")]
    RenderFailed { page: usize, detail: String },

    /// LLM call failed after retries.
    #[error("Page {page}: LLM call failed after {retries} retries: {detail}")]
    LlmFailed {
        page: usize,
        retries: u8,
        detail: String,
    },

    /// LLM call timed out.
    #[error("Page {page}: LLM call timed out after {secs}s")]
    Timeout { page: usize, secs: u64 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_failure_display() {
        let e = Pdf2MdError::PartialFailure {
            success: 9,
            failed: 1,
            total: 10,
        };
        let msg = e.to_string();
        assert!(msg.contains("1/10"), "got: {msg}");
    }

    #[test]
    fn rate_limit_display_with_retry() {
        let e = Pdf2MdError::RateLimitExceeded {
            provider: "openai".into(),
            retry_after_secs: Some(60),
        };
        assert!(e.to_string().contains("openai"));
    }

    #[test]
    fn rate_limit_display_without_retry() {
        let e = Pdf2MdError::RateLimitExceeded {
            provider: "gemini".into(),
            retry_after_secs: None,
        };
        assert!(e.to_string().contains("gemini"));
    }

    #[test]
    fn api_timeout_display() {
        let e = Pdf2MdError::ApiTimeout {
            page: 3,
            elapsed_ms: 5000,
        };
        assert!(e.to_string().contains("5000ms"));
        assert!(e.to_string().contains("page 3"));
    }

    #[test]
    fn auth_error_display() {
        let e = Pdf2MdError::AuthError {
            provider: "anthropic".into(),
            detail: "invalid key".into(),
        };
        assert!(e.to_string().contains("anthropic"));
        assert!(e.to_string().contains("invalid key"));
    }
}
