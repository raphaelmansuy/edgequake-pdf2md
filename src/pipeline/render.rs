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

use crate::config::ConversionConfig;
use crate::error::Pdf2MdError;
use crate::output::DocumentMetadata;
use image::DynamicImage;
use pdfium_render::prelude::*;
use std::path::Path;
use tracing::{debug, info, warn};

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
    let pdfium = Pdfium::default();

    let document = pdfium.load_pdf_from_file(pdf_path, password).map_err(|e| {
        let err_str = format!("{:?}", e);
        if err_str.contains("Password") || err_str.contains("password") {
            if password.is_some() {
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
    })?;

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
    let pdfium = Pdfium::default();

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
