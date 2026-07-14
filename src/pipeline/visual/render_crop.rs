//! Render a PDF-space bbox to a cropped DynamicImage (SPEC-049).

use image::{DynamicImage, GenericImageView};
use pdfium_render::prelude::*;

use super::types::BBox;

/// Render longest edge for region crops.
pub const REGION_MAX_PIXELS: i32 = 2000;

/// Render the page once (shared across all crops on that page).
pub fn render_page_image(page: &PdfPage<'_>) -> Option<DynamicImage> {
    let render_config = region_render_config();
    let bitmap = page.render_with_config(&render_config).ok()?;
    let image = bitmap.as_image();
    let (w, h) = image.dimensions();
    if w == 0 || h == 0 {
        None
    } else {
        Some(image)
    }
}

pub fn region_render_config() -> PdfRenderConfig {
    PdfRenderConfig::new()
        .set_target_width(REGION_MAX_PIXELS)
        .set_maximum_height(REGION_MAX_PIXELS)
}

/// Crop a previously rendered page image to `bbox` (PDF space).
pub fn crop_rendered_page(
    page: &PdfPage<'_>,
    page_image: &DynamicImage,
    bbox: BBox,
) -> Option<DynamicImage> {
    let render_config = region_render_config();
    let (w, h) = page_image.dimensions();
    let (x0, y0) = page
        .points_to_pixels(
            PdfPoints::new(bbox.0),
            PdfPoints::new(bbox.3),
            &render_config,
        )
        .ok()?;
    let (x1, y1) = page
        .points_to_pixels(
            PdfPoints::new(bbox.2),
            PdfPoints::new(bbox.1),
            &render_config,
        )
        .ok()?;

    let left = x0.min(x1).max(0);
    let top = y0.min(y1).max(0);
    let right = x0.max(x1).min(w as i32);
    let bottom = y0.max(y1).min(h as i32);
    if right <= left + 8 || bottom <= top + 8 {
        return None;
    }
    let cw = (right - left) as u32;
    let ch = (bottom - top) as u32;
    Some(page_image.crop_imm(left as u32, top as u32, cw, ch))
}

/// Convenience: render page + crop (prefer batching via [`render_page_image`]).
#[allow(dead_code)] // public helper for callers that crop a single bbox
pub fn render_bbox_crop(page: &PdfPage<'_>, bbox: BBox) -> Option<DynamicImage> {
    let image = render_page_image(page)?;
    crop_rendered_page(page, &image, bbox)
}
