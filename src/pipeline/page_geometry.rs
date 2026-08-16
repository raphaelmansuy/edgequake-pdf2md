//! Page MediaBox / CropBox / Rotate (SPEC-128 overlay SSOT).

use pdfium_render::prelude::*;

use crate::error::Pdf2MdError;
use crate::pipeline::render::get_pdfium;

/// Displayed page box in PDF user space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageMediaBox {
    /// 1-indexed page number.
    pub page_num: usize,
    pub width_pt: f32,
    pub height_pt: f32,
    /// ISO `/Rotate` (0, 90, 180, 270).
    pub rotation: i16,
    /// CropBox when it differs from MediaBox: `(x0,y0,x1,y1)`.
    pub cropbox: Option<(f32, f32, f32, f32)>,
}

/// Extract per-page media geometry from PDF bytes (blocking, no raster).
pub fn extract_page_media_boxes_from_bytes(
    pdf_bytes: &[u8],
    password: Option<&str>,
) -> Result<Vec<PageMediaBox>, Pdf2MdError> {
    let pdfium = get_pdfium()?;
    let document = pdfium
        .load_pdf_from_byte_slice(pdf_bytes, password)
        .map_err(|e| Pdf2MdError::Internal(format!("Failed to open PDF for page geometry: {e}")))?;
    extract_from_document(&document)
}

fn extract_from_document(document: &PdfDocument<'_>) -> Result<Vec<PageMediaBox>, Pdf2MdError> {
    let mut out = Vec::with_capacity(document.pages().len() as usize);
    for (idx0, page) in document.pages().iter().enumerate() {
        let width_pt = page.width().value.max(1.0);
        let height_pt = page.height().value.max(1.0);
        let rotation = page.rotation().map(|r| match r {
            PdfPageRenderRotation::None => 0,
            PdfPageRenderRotation::Degrees90 => 90,
            PdfPageRenderRotation::Degrees180 => 180,
            PdfPageRenderRotation::Degrees270 => 270,
        }).unwrap_or(0);
        out.push(PageMediaBox {
            page_num: idx0 + 1,
            width_pt,
            height_pt,
            rotation,
            cropbox: None,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_pdf() {
        let err = extract_page_media_boxes_from_bytes(b"not-a-pdf", None);
        assert!(err.is_err());
    }
}
