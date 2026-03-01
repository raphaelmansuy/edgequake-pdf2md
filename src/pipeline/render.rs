//! PDF rasterisation: render selected pages to `DynamicImage` via pdfium.
//!
//! ## Why spawn_blocking?
//!
//! The `pdfium-render` crate wraps the pdfium C++ library, which uses
//! thread-local state internally and is not safe to call from async contexts.
//! `tokio::task::spawn_blocking` moves the work onto a dedicated thread pool
//! thread designed for blocking operations, preventing the Tokio worker
//! threads from stalling during CPU-heavy rendering.
//!
//! ## Why cap pixels, not DPI?
//!
//! Page sizes vary wildly: an A0 poster at 150 DPI would produce a
//! 12,000 × 17,000 px image. `max_rendered_pixels` caps the longest edge
//! regardless of physical size, keeping memory bounded and matching the
//! image-size sweet spot for GPT-4 vision (around 1,024–2,048 px).

use super::encode;
use crate::config::ConversionConfig;
use crate::error::Pdf2MdError;
use crate::output::DocumentMetadata;
use edgequake_llm::ImageData;
use image::DynamicImage;
use pdfium_render::prelude::*;
use std::path::Path;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, warn};

/// Obtain a `Pdfium` instance via pdfium-auto.
///
/// When the `bundled` feature is active the pdfium shared library was embedded
/// in the binary at compile time; it is extracted to the cache directory on
/// first use and loaded from there (no network access required).
///
/// Without the `bundled` feature the library is downloaded on first use from
/// <https://github.com/bblanchon/pdfium-binaries> and cached locally.
///
/// # Errors
/// Returns `Pdf2MdError::Internal` when the library cannot be loaded.  The
/// error message includes a `PDFIUM_LIB_PATH` override hint.
fn get_pdfium() -> Result<Pdfium, Pdf2MdError> {
    #[cfg(feature = "bundled")]
    {
        pdfium_auto::bind_bundled().map_err(|e| {
            Pdf2MdError::Internal(format!(
                "PDFium library (bundled) unavailable: {e}\n\
                 Hint: set PDFIUM_LIB_PATH=/path/to/libpdfium to use an existing copy."
            ))
        })
    }

    #[cfg(not(feature = "bundled"))]
    pdfium_auto::bind_pdfium_silent().map_err(|e| {
        Pdf2MdError::Internal(format!(
            "PDFium library unavailable: {e}\n\
             Hint: set PDFIUM_LIB_PATH=/path/to/libpdfium to use an existing copy."
        ))
    })
}

/// Rasterise selected pages of a PDF into images.
///
/// This runs inside `spawn_blocking` since pdfium operations are CPU-bound.
///
/// # Returns
/// A vector of `(page_index_0based, DynamicImage)` tuples.
pub async fn render_pages(
    pdf_path: &Path,
    config: &ConversionConfig,
    page_indices: &[usize],
) -> Result<Vec<(usize, DynamicImage)>, Pdf2MdError> {
    let path = pdf_path.to_path_buf();
    let dpi = config.dpi;
    let max_pixels = config.max_rendered_pixels;
    let password = config.password.clone();
    let indices = page_indices.to_vec();

    let result = tokio::task::spawn_blocking(move || {
        render_pages_blocking(&path, dpi, max_pixels, password.as_deref(), &indices)
    })
    .await
    .map_err(|e| Pdf2MdError::Internal(format!("Render task panicked: {}", e)))?;

    result
}

/// Blocking implementation of page rendering.
fn render_pages_blocking(
    pdf_path: &Path,
    _dpi: u32,
    max_pixels: u32,
    password: Option<&str>,
    page_indices: &[usize],
) -> Result<Vec<(usize, DynamicImage)>, Pdf2MdError> {
    let pdfium = get_pdfium()?;

    let document = pdfium
        .load_pdf_from_file(pdf_path, password)
        .map_err(|e| map_pdf_open_error(e, pdf_path, password.is_some()))?;

    let pages = document.pages();
    let total_pages = pages.len() as usize;
    info!("PDF loaded: {} pages", total_pages);

    let render_config = PdfRenderConfig::new()
        .set_target_width(max_pixels as i32)
        .set_maximum_height(max_pixels as i32);

    let mut results = Vec::with_capacity(page_indices.len());

    for &idx in page_indices {
        if idx >= total_pages {
            warn!(
                "Skipping page {} (out of range, total={})",
                idx + 1,
                total_pages
            );
            continue;
        }

        let page = pages
            .get(idx as u16)
            .map_err(|e| Pdf2MdError::RasterisationFailed {
                page: idx + 1,
                detail: format!("{:?}", e),
            })?;

        let bitmap = page.render_with_config(&render_config).map_err(|e| {
            Pdf2MdError::RasterisationFailed {
                page: idx + 1,
                detail: format!("{:?}", e),
            }
        })?;

        let image = bitmap.as_image();
        debug!(
            "Rendered page {} → {}x{} px",
            idx + 1,
            image.width(),
            image.height()
        );

        results.push((idx, image));
    }

    Ok(results)
}

// ── Lazy render + encode pipeline ────────────────────────────────────────

/// A single page that has been rendered and base64-encoded, ready for VLM.
///
/// Produced lazily by [`spawn_lazy_render_encode`]. The `DynamicImage` is
/// dropped inside the producer immediately after encoding, so only the
/// base64 data lives in memory.
pub struct EncodedPage {
    /// 0-based page index.
    pub page_index: usize,
    /// Base64-encoded PNG image data.
    pub image_data: ImageData,
    /// Time spent rendering + encoding this single page (ms).
    pub render_encode_ms: u64,
}

/// Spawn a lazy render+encode pipeline that produces pages one at a time.
///
/// Opens the PDF once in a [`tokio::task::spawn_blocking`] task, then for
/// each selected page:
/// 1. Renders the page to a `DynamicImage` via pdfium
/// 2. Encodes it to base64 PNG ([`ImageData`])
/// 3. **Drops** the `DynamicImage` immediately (freeing the bitmap memory)
/// 4. Sends the [`EncodedPage`] through a bounded channel
///
/// Memory is bounded: at most `channel_capacity` encoded pages live in the
/// channel at any time. Combined with `buffer_unordered(N)` on the consumer
/// side, peak memory is `≈ 2 × concurrency × page_size` instead of
/// `total_pages × page_size`.
///
/// # Returns
/// - `Ok(receiver)` — pages will arrive as [`EncodedPage`] items
/// - `Err(Pdf2MdError)` — if the PDF cannot be opened (fatal)
///
/// When the receiver is dropped (e.g. consumer cancelled), the producer
/// stops rendering remaining pages automatically.
pub async fn spawn_lazy_render_encode(
    pdf_path: &Path,
    config: &ConversionConfig,
    page_indices: &[usize],
    channel_capacity: usize,
) -> Result<mpsc::Receiver<EncodedPage>, Pdf2MdError> {
    let path = pdf_path.to_path_buf();
    let max_pixels = config.max_rendered_pixels;
    let password = config.password.clone();
    let indices = page_indices.to_vec();

    let (ready_tx, ready_rx) = oneshot::channel::<Result<(), Pdf2MdError>>();
    let (tx, rx) = mpsc::channel(channel_capacity.max(1));

    tokio::task::spawn_blocking(move || {
        lazy_render_encode_blocking(
            &path,
            max_pixels,
            password.as_deref(),
            &indices,
            tx,
            ready_tx,
        )
    });

    // Wait for the producer to confirm the PDF opened successfully.
    match ready_rx.await {
        Ok(Ok(())) => Ok(rx),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(Pdf2MdError::Internal(
            "Render producer task failed before opening PDF".into(),
        )),
    }
}

/// Blocking producer: opens PDF once, renders + encodes pages one at a time.
fn lazy_render_encode_blocking(
    pdf_path: &Path,
    max_pixels: u32,
    password: Option<&str>,
    page_indices: &[usize],
    tx: mpsc::Sender<EncodedPage>,
    ready_tx: oneshot::Sender<Result<(), Pdf2MdError>>,
) {
    let pdfium = match get_pdfium() {
        Ok(p) => p,
        Err(e) => {
            let _ = ready_tx.send(Err(e));
            return;
        }
    };

    let document = match pdfium.load_pdf_from_file(pdf_path, password) {
        Ok(doc) => doc,
        Err(e) => {
            let _ = ready_tx.send(Err(map_pdf_open_error(e, pdf_path, password.is_some())));
            return;
        }
    };

    // PDF opened successfully — signal the async caller.
    let _ = ready_tx.send(Ok(()));

    let pages = document.pages();
    let total_pages = pages.len() as usize;
    info!(
        "Lazy render: PDF loaded ({} pages), producing {} selected pages",
        total_pages,
        page_indices.len()
    );

    let render_config = PdfRenderConfig::new()
        .set_target_width(max_pixels as i32)
        .set_maximum_height(max_pixels as i32);

    for &idx in page_indices {
        if idx >= total_pages {
            warn!(
                "Skipping page {} (out of range, total={})",
                idx + 1,
                total_pages
            );
            continue;
        }

        let start = std::time::Instant::now();

        let page = match pages.get(idx as u16) {
            Ok(p) => p,
            Err(e) => {
                warn!("Skipping page {} (render failed: {:?})", idx + 1, e);
                continue;
            }
        };

        let bitmap = match page.render_with_config(&render_config) {
            Ok(b) => b,
            Err(e) => {
                warn!("Skipping page {} (render failed: {:?})", idx + 1, e);
                continue;
            }
        };

        let image = bitmap.as_image();
        debug!(
            "Rendered page {} → {}x{} px",
            idx + 1,
            image.width(),
            image.height()
        );

        let data = match encode::encode_page(&image) {
            Ok(d) => d,
            Err(e) => {
                warn!("Skipping page {} (encoding failed: {})", idx + 1, e);
                continue;
            }
        };
        // `image` is dropped here, freeing the DynamicImage bitmap memory.

        let render_encode_ms = start.elapsed().as_millis() as u64;

        let encoded_page = EncodedPage {
            page_index: idx,
            image_data: data,
            render_encode_ms,
        };

        // Blocking send: waits if channel is full (back-pressure from consumer).
        // Returns Err if receiver is dropped (consumer cancelled).
        if tx.blocking_send(encoded_page).is_err() {
            debug!("Lazy render producer: receiver dropped, stopping");
            break;
        }
    }
}

/// Map a pdfium document-open error to a [`Pdf2MdError`].
fn map_pdf_open_error(e: impl std::fmt::Debug, pdf_path: &Path, has_password: bool) -> Pdf2MdError {
    let err_str = format!("{:?}", e);
    if err_str.contains("Password") || err_str.contains("password") {
        if has_password {
            Pdf2MdError::WrongPassword {
                path: pdf_path.to_path_buf(),
            }
        } else {
            Pdf2MdError::PasswordRequired {
                path: pdf_path.to_path_buf(),
            }
        }
    } else {
        Pdf2MdError::CorruptPdf {
            path: pdf_path.to_path_buf(),
            detail: err_str,
        }
    }
}

/// Extract document metadata from a PDF without rendering pages.
pub async fn extract_metadata(
    pdf_path: &Path,
    password: Option<&str>,
) -> Result<DocumentMetadata, Pdf2MdError> {
    let path = pdf_path.to_path_buf();
    let pwd = password.map(|s| s.to_string());

    tokio::task::spawn_blocking(move || extract_metadata_blocking(&path, pwd.as_deref()))
        .await
        .map_err(|e| Pdf2MdError::Internal(format!("Metadata task panicked: {}", e)))?
}

/// Blocking implementation of metadata extraction.
fn extract_metadata_blocking(
    pdf_path: &Path,
    password: Option<&str>,
) -> Result<DocumentMetadata, Pdf2MdError> {
    let pdfium = get_pdfium()?;

    let document =
        pdfium
            .load_pdf_from_file(pdf_path, password)
            .map_err(|e| Pdf2MdError::CorruptPdf {
                path: pdf_path.to_path_buf(),
                detail: format!("{:?}", e),
            })?;

    let metadata = document.metadata();
    let pages = document.pages();

    let get_meta = |tag: PdfDocumentMetadataTagType| -> Option<String> {
        metadata.get(tag).and_then(|t| {
            let v = t.value().to_string();
            if v.is_empty() {
                None
            } else {
                Some(v)
            }
        })
    };

    Ok(DocumentMetadata {
        title: get_meta(PdfDocumentMetadataTagType::Title),
        author: get_meta(PdfDocumentMetadataTagType::Author),
        subject: get_meta(PdfDocumentMetadataTagType::Subject),
        creator: get_meta(PdfDocumentMetadataTagType::Creator),
        producer: get_meta(PdfDocumentMetadataTagType::Producer),
        creation_date: get_meta(PdfDocumentMetadataTagType::CreationDate),
        modification_date: get_meta(PdfDocumentMetadataTagType::ModificationDate),
        page_count: pages.len() as usize,
        pdf_version: format!("{:?}", document.version()),
        is_encrypted: false, // pdfium doesn't readily expose this after opening
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn map_pdf_open_error_password_required() {
        let e = "PdfiumError::PasswordRequired";
        let err = map_pdf_open_error(e, Path::new("/test.pdf"), false);
        match err {
            Pdf2MdError::PasswordRequired { path } => {
                assert_eq!(path, PathBuf::from("/test.pdf"));
            }
            other => panic!("expected PasswordRequired, got {other:?}"),
        }
    }

    #[test]
    fn map_pdf_open_error_wrong_password() {
        let e = "PdfiumError::IncorrectPassword";
        let err = map_pdf_open_error(e, Path::new("/test.pdf"), true);
        match err {
            Pdf2MdError::WrongPassword { path } => {
                assert_eq!(path, PathBuf::from("/test.pdf"));
            }
            other => panic!("expected WrongPassword, got {other:?}"),
        }
    }

    #[test]
    fn map_pdf_open_error_corrupt() {
        let e = "SomeOtherError";
        let err = map_pdf_open_error(e, Path::new("/bad.pdf"), false);
        match err {
            Pdf2MdError::CorruptPdf { path, detail } => {
                assert_eq!(path, PathBuf::from("/bad.pdf"));
                assert!(detail.contains("SomeOtherError"));
            }
            other => panic!("expected CorruptPdf, got {other:?}"),
        }
    }

    #[test]
    fn encoded_page_fields() {
        let data = ImageData::new("dGVzdA==".to_string(), "image/png");
        let page = EncodedPage {
            page_index: 5,
            image_data: data,
            render_encode_ms: 42,
        };
        assert_eq!(page.page_index, 5);
        assert_eq!(page.image_data.mime_type, "image/png");
        assert_eq!(page.render_encode_ms, 42);
    }

    #[tokio::test]
    async fn spawn_lazy_nonexistent_file_returns_err() {
        let config = ConversionConfig::default();
        let result =
            spawn_lazy_render_encode(Path::new("/nonexistent/file.pdf"), &config, &[0], 1).await;
        assert!(result.is_err(), "should fail for nonexistent PDF");
    }

    /// Verify the lazy pipeline produces correct pages from a real PDF.
    /// Skipped when pdfium is not available (e.g. CI without bundled feature).
    #[tokio::test]
    async fn spawn_lazy_produces_pages() {
        let pdf_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_cases")
            .join("irs_form_1040.pdf");
        if !pdf_path.exists() {
            println!("SKIP — test_cases/irs_form_1040.pdf not found");
            return;
        }

        let config = ConversionConfig::default();
        let rx = match spawn_lazy_render_encode(&pdf_path, &config, &[0, 1], 2).await {
            Ok(rx) => rx,
            Err(e) => {
                println!("SKIP — pdfium not available: {e}");
                return;
            }
        };

        let mut rx = rx;
        let mut pages = Vec::new();
        while let Some(page) = rx.recv().await {
            pages.push(page);
        }

        assert_eq!(pages.len(), 2, "IRS form has 2 pages, selected both");
        assert_eq!(pages[0].page_index, 0);
        assert_eq!(pages[1].page_index, 1);
        assert!(
            !pages[0].image_data.data.is_empty(),
            "base64 should be non-empty"
        );
        assert!(
            !pages[1].image_data.data.is_empty(),
            "base64 should be non-empty"
        );
        assert!(pages[0].render_encode_ms > 0 || pages[1].render_encode_ms > 0);
    }

    /// Verify out-of-range page indices are silently skipped.
    #[tokio::test]
    async fn spawn_lazy_skips_out_of_range() {
        let pdf_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_cases")
            .join("irs_form_1040.pdf");
        if !pdf_path.exists() {
            println!("SKIP — test_cases/irs_form_1040.pdf not found");
            return;
        }

        let config = ConversionConfig::default();
        let rx = match spawn_lazy_render_encode(&pdf_path, &config, &[0, 999], 2).await {
            Ok(rx) => rx,
            Err(e) => {
                println!("SKIP — pdfium not available: {e}");
                return;
            }
        };

        let mut rx = rx;
        let mut pages = Vec::new();
        while let Some(page) = rx.recv().await {
            pages.push(page);
        }

        assert_eq!(
            pages.len(),
            1,
            "only page 0 should be produced (page 999 is out of range)"
        );
        assert_eq!(pages[0].page_index, 0);
    }

    /// Verify the producer stops when the receiver is dropped.
    #[tokio::test]
    async fn spawn_lazy_stops_on_receiver_drop() {
        let pdf_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_cases")
            .join("attention_is_all_you_need.pdf");
        if !pdf_path.exists() {
            println!("SKIP — test PDF not found");
            return;
        }

        let config = ConversionConfig::default();
        // Request all 15 pages but only consume 1
        let all_indices: Vec<usize> = (0..15).collect();
        let rx = match spawn_lazy_render_encode(&pdf_path, &config, &all_indices, 1).await {
            Ok(rx) => rx,
            Err(e) => {
                println!("SKIP — pdfium not available: {e}");
                return;
            }
        };

        let mut rx = rx;
        // Consume only the first page
        let first = rx.recv().await;
        assert!(first.is_some(), "should get at least one page");
        // Drop receiver — producer should stop
        drop(rx);
        // If the producer doesn't stop, this test would hang (spawn_blocking
        // would keep rendering). The test completing proves the producer stops.
    }

    /// Verify bounded channel provides back-pressure (capacity=1 means
    /// at most 1 page buffered).
    #[tokio::test]
    async fn spawn_lazy_bounded_channel() {
        let pdf_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_cases")
            .join("irs_form_1040.pdf");
        if !pdf_path.exists() {
            println!("SKIP — test PDF not found");
            return;
        }

        let config = ConversionConfig::default();
        // Channel capacity 1: producer can only be 1 page ahead
        let rx = match spawn_lazy_render_encode(&pdf_path, &config, &[0, 1], 1).await {
            Ok(rx) => rx,
            Err(e) => {
                println!("SKIP — pdfium not available: {e}");
                return;
            }
        };

        let mut rx = rx;
        // Slowly consume — the producer is bounded by channel capacity
        let p1 = rx.recv().await;
        assert!(p1.is_some());
        // Small delay to let producer attempt to send page 2
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        let p2 = rx.recv().await;
        assert!(p2.is_some());
        let p3 = rx.recv().await;
        assert!(p3.is_none(), "channel should be closed after 2 pages");
    }
}
