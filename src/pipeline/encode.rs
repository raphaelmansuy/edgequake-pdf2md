//! Image encoding: `DynamicImage` → base64 PNG wrapped in `ImageData`.
//!
//! VLM APIs (OpenAI, Anthropic, Gemini) accept images as base64 data-URIs
//! embedded in the JSON request body. PNG is chosen over JPEG because it is
//! lossless — text crispness matters far more than file size for OCR accuracy.
//! `detail: "high"` instructs GPT-4-class models to use the full 768-token
//! image tile budget; without it fine print and small tables are lost.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use edgequake_llm::ImageData;
use image::DynamicImage;
use std::io::Cursor;
use tracing::debug;

/// Encode a rasterised page as a base64 PNG ready for the VLM API.
///
/// ## Why PNG?
/// Lossless compression preserves text crispness. JPEG artefacts on rendered
/// text confuse vision models and degrade OCR accuracy at low DPI.
///
/// ## Why `detail: "high"`?
/// OpenAI's tiling algorithm divides images into 512 px tiles. `detail: "high"`
/// enables up to 10 tiles (765 tokens each), allowing fine print, small tables,
/// and math notation to be seen. `detail: "low"` forces a single 512 px
/// overview tile and loses all fine structure.
pub fn encode_page(img: &DynamicImage) -> Result<ImageData, image::ImageError> {
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)?;

    let b64 = STANDARD.encode(&buf);
    debug!("Encoded image → {} bytes base64", b64.len());

    Ok(ImageData::new(b64, "image/png").with_detail("high"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn encode_small_image() {
        let img = DynamicImage::ImageRgba8(RgbaImage::from_pixel(10, 10, Rgba([255, 0, 0, 255])));
        let data = encode_page(&img).expect("encode should succeed");
        assert_eq!(data.mime_type, "image/png");
        assert!(!data.data.is_empty());
        // Verify it's valid base64
        let decoded = STANDARD.decode(&data.data).expect("valid base64");
        assert!(!decoded.is_empty());
    }
}
