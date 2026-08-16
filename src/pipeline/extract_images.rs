//! Extract embedded PDF ImageXObjects (figure-bounded bitmaps).
//!
//! First principle: VLM analysis of figures/charts/illustrations must target
//! the **image object** in the PDF, not a full-page raster. Full-page renders
//! (`render_pages`) remain for page OCR / viewer context.
//!
//! Reuses the process-wide Pdfium singleton from [`super::render`] (DRY / no
//! second `FPDF_InitLibrary`).

use image::DynamicImage;
use pdfium_render::prelude::*;
use tracing::{debug, info};

use crate::error::Pdf2MdError;
use crate::pipeline::render::get_pdfium;
use crate::pipeline::visual::{aspect_ok, bbox_area, MAX_AREA_FRAC};

/// Skip decorative bullets / 1px spacers.
const MIN_FIGURE_EDGE_PX: u32 = 24;

/// One embedded figure decoded from a PDF ImageXObject.
#[derive(Debug, Clone)]
pub struct EmbeddedImage {
    /// 1-indexed page number.
    pub page_num: usize,
    /// 1-indexed image index on that page (stable for filenames).
    pub index: usize,
    /// PDF-space bounding box `(left, bottom, right, top)`.
    pub bbox: (f32, f32, f32, f32),
    pub width: u32,
    pub height: u32,
    pub image: DynamicImage,
}

/// Extract meaningful ImageXObjects from a PDF file (blocking).
pub fn extract_embedded_images_from_path(
    pdf_path: &std::path::Path,
    password: Option<&str>,
) -> Result<Vec<EmbeddedImage>, Pdf2MdError> {
    if !pdf_path.exists() {
        return Err(Pdf2MdError::FileNotFound {
            path: pdf_path.to_path_buf(),
        });
    }
    let pdfium = get_pdfium()?;
    let document = pdfium
        .load_pdf_from_file(pdf_path, password)
        .map_err(|e| Pdf2MdError::Internal(format!("Failed to open PDF for image extract: {e}")))?;
    extract_from_document(&document)
}

/// Extract meaningful ImageXObjects from PDF bytes (blocking).
pub fn extract_embedded_images_from_bytes(
    pdf_bytes: &[u8],
    password: Option<&str>,
) -> Result<Vec<EmbeddedImage>, Pdf2MdError> {
    let pdfium = get_pdfium()?;
    let document = pdfium
        .load_pdf_from_byte_slice(pdf_bytes, password)
        .map_err(|e| Pdf2MdError::Internal(format!("Failed to open PDF for image extract: {e}")))?;
    extract_from_document(&document)
}

/// Async wrapper: runs extract on the blocking pool (pdfium is not async-safe).
pub async fn extract_embedded_images(
    pdf_bytes: &[u8],
    password: Option<&str>,
) -> Result<Vec<EmbeddedImage>, Pdf2MdError> {
    let bytes = pdf_bytes.to_vec();
    let password = password.map(str::to_owned);
    tokio::task::spawn_blocking(move || {
        extract_embedded_images_from_bytes(&bytes, password.as_deref())
    })
    .await
    .map_err(|e| Pdf2MdError::Internal(format!("Image extract task panicked: {e}")))?
}

fn extract_from_document(document: &PdfDocument<'_>) -> Result<Vec<EmbeddedImage>, Pdf2MdError> {
    let mut images = Vec::new();
    for (page_idx0, page) in document.pages().iter().enumerate() {
        let page_num = page_idx0 + 1;
        let page_area = page.width().value.max(1.0) * page.height().value.max(1.0);
        let mut img_idx = 0usize;
        for object in page.objects().iter() {
            let Some(image_obj) = object.as_image_object() else {
                continue;
            };
            img_idx += 1;
            let raw = match image_obj.get_processed_image(document) {
                Ok(img) => img,
                Err(e1) => match image_obj.get_raw_image() {
                    Ok(img) => img,
                    Err(e2) => {
                        debug!(
                            page_num,
                            img_idx,
                            error_processed = %e1,
                            error_raw = %e2,
                            "Skipping ImageXObject (decode failed)"
                        );
                        continue;
                    }
                },
            };
            let width = raw.width();
            let height = raw.height();
            if width < MIN_FIGURE_EDGE_PX || height < MIN_FIGURE_EDGE_PX {
                debug!(
                    page_num,
                    img_idx, width, height, "Skipping tiny ImageXObject"
                );
                continue;
            }
            let bbox = object_bbox(&object);
            let frac = bbox_area(bbox) / page_area.max(1.0);
            if frac > MAX_AREA_FRAC || !aspect_ok(bbox) {
                debug!(
                    page_num,
                    img_idx,
                    frac,
                    "Skipping ImageXObject (SPEC-128 full-page/aspect gate)"
                );
                continue;
            }
            images.push(EmbeddedImage {
                page_num,
                index: img_idx,
                bbox,
                width,
                height,
                image: raw,
            });
        }
    }
    info!(
        images = images.len(),
        pages = document.pages().len(),
        "Extracted embedded PDF ImageXObjects"
    );
    Ok(images)
}

fn object_bbox(object: &PdfPageObject<'_>) -> (f32, f32, f32, f32) {
    match object.bounds() {
        Ok(bounds) => {
            let rect = bounds.to_rect();
            (
                rect.left().value,
                rect.bottom().value,
                rect.right().value,
                rect.top().value,
            )
        }
        Err(_) => (0.0, 0.0, 0.0, 0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample_pdf() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_cases/embedded_figure_sample.pdf")
    }

    #[test]
    fn extracts_object_bitmap_not_full_page() {
        let path = sample_pdf();
        if !path.exists() {
            eprintln!("fixture missing: {path:?}");
            return;
        }
        let imgs = match extract_embedded_images_from_path(&path, None) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("pdfium unavailable: {e}");
                return;
            }
        };
        assert!(!imgs.is_empty(), "expected ≥1 ImageXObject");
        let img = &imgs[0];
        assert_eq!(img.page_num, 1);
        assert!(
            img.width <= 80 && img.height <= 80,
            "object pixels expected, got {}x{}",
            img.width,
            img.height
        );
        let (l, b, r, t) = img.bbox;
        let bw = (r - l).abs();
        let bh = (t - b).abs();
        assert!(
            bw < 500.0 && bh < 700.0,
            "bbox should be figure-sized, got {bw}x{bh}"
        );
    }
}
