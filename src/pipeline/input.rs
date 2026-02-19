//! Input resolution: normalise a user-supplied path or URL to a local file.
//!
//! ## Why download to a temp file?
//!
//! pdfium requires a file-system path — it cannot stream from a byte buffer.
//! Downloading to a `TempDir` gives us a path pdfium can open while ensuring
//! cleanup happens automatically when `ResolvedInput` is dropped, even if
//! the process panics. We validate the PDF magic bytes (`%PDF`) before
//! returning so callers get a meaningful error rather than a pdfium crash.

use crate::error::Pdf2MdError;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tracing::{debug, info};

/// The resolved input — either a local path or a downloaded temp file.
pub enum ResolvedInput {
    /// Input was already a local file.
    Local(PathBuf),
    /// Input was a URL; PDF downloaded to a temp directory.
    /// The `TempDir` is kept alive to prevent cleanup until processing completes.
    Downloaded { path: PathBuf, _temp_dir: TempDir },
}

impl ResolvedInput {
    /// Get the path to the PDF file regardless of how it was resolved.
    pub fn path(&self) -> &Path {
        match self {
            ResolvedInput::Local(p) => p,
            ResolvedInput::Downloaded { path, .. } => path,
        }
    }
}

/// Check if the input string looks like a URL.
pub fn is_url(input: &str) -> bool {
    input.starts_with("http://") || input.starts_with("https://")
}

/// Resolve the input string to a local PDF file path.
///
/// If the input is a URL, download it to a temporary directory.
/// If the input is a local file, validate it exists and is readable.
pub async fn resolve_input(input: &str, timeout_secs: u64) -> Result<ResolvedInput, Pdf2MdError> {
    if is_url(input) {
        download_url(input, timeout_secs).await
    } else {
        resolve_local(input)
    }
}

/// Resolve a local file path, validating existence and PDF magic bytes.
fn resolve_local(path_str: &str) -> Result<ResolvedInput, Pdf2MdError> {
    let path = PathBuf::from(path_str);

    if !path.exists() {
        return Err(Pdf2MdError::FileNotFound { path });
    }

    // Check read permission by attempting to open
    match std::fs::File::open(&path) {
        Ok(mut f) => {
            // Verify PDF magic bytes
            use std::io::Read;
            let mut magic = [0u8; 4];
            if f.read_exact(&mut magic).is_ok() && &magic != b"%PDF" {
                return Err(Pdf2MdError::NotAPdf { path, magic });
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            return Err(Pdf2MdError::PermissionDenied { path });
        }
        Err(_) => {
            return Err(Pdf2MdError::FileNotFound { path });
        }
    }

    debug!("Resolved local PDF: {}", path.display());
    Ok(ResolvedInput::Local(path))
}

/// Download a URL to a temporary directory and return the path.
async fn download_url(url: &str, timeout_secs: u64) -> Result<ResolvedInput, Pdf2MdError> {
    info!("Downloading PDF from: {}", url);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| Pdf2MdError::DownloadFailed {
            url: url.to_string(),
            reason: e.to_string(),
        })?;

    let response = client.get(url).send().await.map_err(|e| {
        if e.is_timeout() {
            Pdf2MdError::DownloadTimeout {
                url: url.to_string(),
                secs: timeout_secs,
            }
        } else {
            Pdf2MdError::DownloadFailed {
                url: url.to_string(),
                reason: e.to_string(),
            }
        }
    })?;

    if !response.status().is_success() {
        return Err(Pdf2MdError::DownloadFailed {
            url: url.to_string(),
            reason: format!("HTTP {}", response.status()),
        });
    }

    // Extract filename from URL or Content-Disposition
    let filename = extract_filename(url, &response);

    let temp_dir = TempDir::new().map_err(|e| Pdf2MdError::Internal(e.to_string()))?;
    let file_path = temp_dir.path().join(&filename);

    let bytes = response
        .bytes()
        .await
        .map_err(|e| Pdf2MdError::DownloadFailed {
            url: url.to_string(),
            reason: e.to_string(),
        })?;

    tokio::fs::write(&file_path, &bytes)
        .await
        .map_err(|e| Pdf2MdError::Internal(format!("Failed to write temp file: {}", e)))?;

    // Verify PDF magic bytes
    if bytes.len() >= 4 && &bytes[..4] != b"%PDF" {
        let mut magic = [0u8; 4];
        magic.copy_from_slice(&bytes[..4]);
        return Err(Pdf2MdError::NotAPdf {
            path: file_path,
            magic,
        });
    }

    info!("Downloaded to: {}", file_path.display());

    Ok(ResolvedInput::Downloaded {
        path: file_path,
        _temp_dir: temp_dir,
    })
}

/// Extract a reasonable filename from the URL or response headers.
fn extract_filename(url: &str, _response: &reqwest::Response) -> String {
    // Try URL path
    if let Ok(parsed) = reqwest::Url::parse(url) {
        if let Some(mut segments) = parsed.path_segments() {
            if let Some(last) = segments.next_back() {
                if !last.is_empty() && last.contains('.') {
                    return last.to_string();
                }
            }
        }
    }

    "downloaded.pdf".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_url() {
        assert!(is_url("https://example.com/doc.pdf"));
        assert!(is_url("http://example.com/doc.pdf"));
        assert!(!is_url("/tmp/doc.pdf"));
        assert!(!is_url("doc.pdf"));
        assert!(!is_url(""));
    }

    // NOTE: extract_filename requires a reqwest::Response which cannot
    // be easily constructed in a unit test. It is covered by integration tests.

    #[test]
    fn test_page_selection_to_indices() {
        use crate::config::PageSelection;

        assert_eq!(PageSelection::All.to_indices(5), vec![0, 1, 2, 3, 4]);
        assert_eq!(PageSelection::Single(3).to_indices(5), vec![2]);
        assert_eq!(PageSelection::Single(6).to_indices(5), Vec::<usize>::new());
        assert_eq!(PageSelection::Range(2, 4).to_indices(5), vec![1, 2, 3]);
        assert_eq!(
            PageSelection::Set(vec![1, 3, 5]).to_indices(5),
            vec![0, 2, 4]
        );
        assert_eq!(
            PageSelection::Set(vec![3, 1, 3]).to_indices(5),
            vec![0, 2] // deduplicated and sorted
        );
    }
}
