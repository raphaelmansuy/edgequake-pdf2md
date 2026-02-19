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
    #[error("Failed to bind to pdfium library: {0}\n\n\
Quick fix — run the bundled setup script:\n\
  ./scripts/setup-pdfium.sh\n\n\
Or install manually:\n\
  macOS:   brew install pdfium-chromium  (or download from github.com/bblanchon/pdfium-binaries)\n\
  Linux:   Download from github.com/bblanchon/pdfium-binaries, place libpdfium.so next to the binary or in /usr/local/lib\n\
  Windows: Download pdfium.dll from github.com/bblanchon/pdfium-binaries, place next to pdf2md.exe\n\n\
Then set the library path:\n\
  macOS:   export DYLD_LIBRARY_PATH=$(pwd)\n\
  Linux:   export LD_LIBRARY_PATH=$(pwd)\n")]
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
