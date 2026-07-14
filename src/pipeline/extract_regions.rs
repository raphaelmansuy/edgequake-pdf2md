//! Caption / visual region crops — facade over SPEC-049 `pipeline::visual`.
//!
//! First principles (049):
//! - L1 object clusters (Form / Image / ruled paths) propose regions.
//! - Captions **label** regions; they do not invent them.
//! - Never return a near-full-page raster as a figure/table.
//!
//! Legacy [`PageRegion`] / [`extract_caption_regions_*`] remain for API stability.

use crate::error::Pdf2MdError;
use crate::pipeline::visual::{
    self, extract_visual_regions, extract_visual_regions_from_bytes,
    extract_visual_regions_from_path, RegionSource, VisualRegion,
};

pub use visual::{RegionKind, MAX_AREA_FRAC, MIN_AREA_FRAC};

/// One caption-/object-anchored page region rendered to a bitmap.
#[derive(Debug, Clone)]
pub struct PageRegion {
    /// 1-indexed page number.
    pub page_num: usize,
    /// 1-indexed index among regions of the same kind on that page.
    pub index: usize,
    pub kind: RegionKind,
    /// Caption label such as `Figure 1` or `Table 1`.
    pub label: String,
    /// PDF-space bounding box `(left, bottom, right, top)`.
    pub bbox: (f32, f32, f32, f32),
    pub width: u32,
    pub height: u32,
    pub image: image::DynamicImage,
    /// Cascade layer that proposed this region (SPEC-049).
    pub source: RegionSource,
    pub has_image: bool,
    pub has_form: bool,
}

impl From<VisualRegion> for PageRegion {
    fn from(v: VisualRegion) -> Self {
        Self {
            page_num: v.page_num,
            index: v.index,
            kind: v.kind,
            label: v.label,
            bbox: v.bbox,
            width: v.width,
            height: v.height,
            image: v.image,
            source: v.source,
            has_image: v.has_image,
            has_form: v.has_form,
        }
    }
}

/// Extract figure/table region crops from PDF bytes (blocking).
pub fn extract_caption_regions_from_bytes(
    pdf_bytes: &[u8],
    password: Option<&str>,
) -> Result<Vec<PageRegion>, Pdf2MdError> {
    Ok(extract_visual_regions_from_bytes(pdf_bytes, password)?
        .into_iter()
        .map(PageRegion::from)
        .collect())
}

/// Extract figure/table region crops from a PDF file (blocking).
pub fn extract_caption_regions_from_path(
    pdf_path: &std::path::Path,
    password: Option<&str>,
) -> Result<Vec<PageRegion>, Pdf2MdError> {
    Ok(extract_visual_regions_from_path(pdf_path, password)?
        .into_iter()
        .map(PageRegion::from)
        .collect())
}

/// Async wrapper around [`extract_caption_regions_from_bytes`].
pub async fn extract_caption_regions(
    pdf_bytes: &[u8],
    password: Option<&str>,
) -> Result<Vec<PageRegion>, Pdf2MdError> {
    Ok(extract_visual_regions(pdf_bytes, password)
        .await?
        .into_iter()
        .map(PageRegion::from)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample_pdf() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_cases/vector_figure_table_sample.pdf")
    }

    #[test]
    fn facade_preserves_region_sized_crops() {
        let path = sample_pdf();
        if !path.exists() {
            eprintln!("fixture missing: {path:?}");
            return;
        }
        let regions = match extract_caption_regions_from_path(&path, None) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("pdfium unavailable: {e}");
                return;
            }
        };
        assert!(!regions.is_empty(), "expected regions from vector sample");
        for r in &regions {
            let area = (r.width as u64) * (r.height as u64);
            assert!(area < 1_200_000, "too large {}x{}", r.width, r.height);
        }
    }
}
