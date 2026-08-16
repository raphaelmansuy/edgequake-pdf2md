//! Deterministic text-page clusters → overlay `paragraph` + derived `column`.
//!
//! No neural net. Uses pdfium `PdfPageText` character quads (SPEC-128 WP-2).
//! `column` is derived from paragraph x-overlap (LAW-128-6).

use pdfium_render::prelude::*;

use super::types::BBox;
use crate::error::Pdf2MdError;
use crate::pipeline::render::get_pdfium;

/// One overlay region from the text channel (not a RAG PNG).
#[derive(Debug, Clone, PartialEq)]
pub struct TextLayoutRegion {
    pub page_num: usize,
    /// Canonical class: `paragraph` or `column`.
    pub class: &'static str,
    /// `l1_paint` for paragraph quads; `derived` for columns.
    pub source: &'static str,
    pub bbox: BBox,
    pub reading_order: i32,
}

/// Extract paragraph + column overlay boxes from PDF bytes (blocking, no raster).
pub fn extract_text_layout_from_bytes(
    pdf_bytes: &[u8],
    password: Option<&str>,
) -> Result<Vec<TextLayoutRegion>, Pdf2MdError> {
    let pdfium = get_pdfium()?;
    let document = pdfium
        .load_pdf_from_byte_slice(pdf_bytes, password)
        .map_err(|e| Pdf2MdError::Internal(format!("Failed to open PDF for text layout: {e}")))?;
    let mut out = Vec::new();
    for (idx0, page) in document.pages().iter().enumerate() {
        out.extend(extract_page_text_layout(&page, idx0 + 1));
    }
    Ok(out)
}

fn extract_page_text_layout(page: &PdfPage<'_>, page_num: usize) -> Vec<TextLayoutRegion> {
    let page_w = page.width().value.max(1.0);
    let page_h = page.height().value.max(1.0);
    let Ok(text) = page.text() else {
        return Vec::new();
    };
    let mut char_boxes = Vec::new();
    for ch in text.chars().iter() {
        let Some(c) = ch.unicode_char() else {
            continue;
        };
        if c.is_whitespace() {
            continue;
        }
        let Ok(rect) = ch.loose_bounds() else {
            continue;
        };
        let bb = (
            rect.left().value,
            rect.bottom().value,
            rect.right().value,
            rect.top().value,
        );
        if (bb.2 - bb.0).abs() < 0.5 || (bb.3 - bb.1).abs() < 0.5 {
            continue;
        }
        char_boxes.push(bb);
    }
    let paragraphs = cluster_paragraphs(&char_boxes, page_w, page_h);
    let columns = derive_columns(&paragraphs, page_w, page_h);
    let mut out = Vec::with_capacity(paragraphs.len() + columns.len());
    for (i, bbox) in paragraphs.into_iter().enumerate() {
        out.push(TextLayoutRegion {
            page_num,
            class: "paragraph",
            source: "l1_paint",
            bbox,
            reading_order: (i as i32) + 1,
        });
    }
    for (i, bbox) in columns.into_iter().enumerate() {
        out.push(TextLayoutRegion {
            page_num,
            class: "column",
            source: "derived",
            bbox,
            reading_order: (i as i32) + 1,
        });
    }
    out
}

/// Cluster character boxes into paragraph boxes (PDF y-up).
pub fn cluster_paragraphs(char_boxes: &[BBox], page_w: f32, page_h: f32) -> Vec<BBox> {
    if char_boxes.is_empty() {
        return Vec::new();
    }
    let mut chars: Vec<BBox> = char_boxes.to_vec();
    chars.sort_by(|a, b| {
        b.3.partial_cmp(&a.3)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
    });
    let lines = cluster_lines(&chars);
    merge_lines_into_paragraphs(&lines, page_w, page_h)
}

fn cluster_lines(chars: &[BBox]) -> Vec<BBox> {
    let mut lines: Vec<BBox> = Vec::new();
    for &ch in chars {
        let mut merged = false;
        for line in &mut lines {
            if y_overlap_frac(ch, *line) >= 0.5 {
                *line = union_bbox(*line, ch);
                merged = true;
                break;
            }
        }
        if !merged {
            lines.push(ch);
        }
    }
    lines
}

fn merge_lines_into_paragraphs(lines: &[BBox], page_w: f32, page_h: f32) -> Vec<BBox> {
    if lines.is_empty() {
        return Vec::new();
    }
    let mut lines = lines.to_vec();
    lines.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
    let median_h = {
        let mut hs: Vec<f32> = lines.iter().map(|l| (l.3 - l.1).abs()).collect();
        hs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        hs[hs.len() / 2].max(8.0)
    };
    let max_gap = median_h * 1.6;
    let mut paras: Vec<BBox> = Vec::new();
    let mut cur = lines[0];
    for &line in lines.iter().skip(1) {
        let gap = (cur.1 - line.3).abs();
        let left_delta = (cur.0 - line.0).abs();
        if gap <= max_gap && left_delta <= 24.0 {
            cur = union_bbox(cur, line);
        } else {
            if paragraph_area_ok(cur, page_w, page_h) {
                paras.push(cur);
            }
            cur = line;
        }
    }
    if paragraph_area_ok(cur, page_w, page_h) {
        paras.push(cur);
    }
    paras
}

fn paragraph_area_ok(bb: BBox, page_w: f32, page_h: f32) -> bool {
    let w = (bb.2 - bb.0).abs();
    let h = (bb.3 - bb.1).abs();
    w >= 40.0 && h >= 10.0 && (w * h) / (page_w * page_h).max(1.0) >= 0.004
}

/// Derive `column` boxes from paragraphs. Skip a single full-width cluster.
pub fn derive_columns(paragraphs: &[BBox], page_w: f32, page_h: f32) -> Vec<BBox> {
    if paragraphs.len() < 2 {
        return Vec::new();
    }
    let n = paragraphs.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut x = i;
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    for i in 0..n {
        for j in (i + 1)..n {
            if iou_x(paragraphs[i], paragraphs[j]) >= 0.6 {
                let a = find(&mut parent, i);
                let b = find(&mut parent, j);
                if a != b {
                    parent[b] = a;
                }
            }
        }
    }
    let mut groups: Vec<Vec<BBox>> = vec![Vec::new(); n];
    for (i, bb) in paragraphs.iter().enumerate() {
        groups[find(&mut parent, i)].push(*bb);
    }
    let mut cols: Vec<BBox> = groups
        .into_iter()
        .filter(|g| !g.is_empty())
        .map(|g| union_all(&g))
        .filter(|bb| {
            let w = (bb.2 - bb.0).abs();
            let h = (bb.3 - bb.1).abs();
            w >= 40.0 && h >= 40.0 && (w * h) / (page_w * page_h).max(1.0) >= 0.02
        })
        .collect();
    if cols.len() == 1 {
        let w = (cols[0].2 - cols[0].0).abs();
        if w > page_w * 0.8 {
            return Vec::new();
        }
    }
    cols.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    cols
}

fn iou_x(a: BBox, b: BBox) -> f32 {
    let overlap = (a.2.min(b.2) - a.0.max(b.0)).max(0.0);
    let union = (a.2.max(b.2) - a.0.min(b.0)).max(1.0);
    overlap / union
}

fn y_overlap_frac(a: BBox, b: BBox) -> f32 {
    let overlap = (a.3.min(b.3) - a.1.max(b.1)).max(0.0);
    let min_h = (a.3 - a.1).abs().min((b.3 - b.1).abs()).max(1.0);
    overlap / min_h
}

fn union_bbox(a: BBox, b: BBox) -> BBox {
    (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3))
}

fn union_all(boxes: &[BBox]) -> BBox {
    boxes
        .iter()
        .copied()
        .reduce(union_bbox)
        .unwrap_or((0.0, 0.0, 0.0, 0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_columns_from_left_right_paragraphs() {
        let page_w = 612.0;
        let page_h = 792.0;
        let paras = vec![
            (36.0, 400.0, 280.0, 720.0),
            (36.0, 80.0, 280.0, 380.0),
            (320.0, 400.0, 576.0, 720.0),
            (320.0, 80.0, 576.0, 380.0),
        ];
        let cols = derive_columns(&paras, page_w, page_h);
        assert_eq!(cols.len(), 2);
        assert!(cols[0].0 < 100.0);
        assert!(cols[1].0 > 250.0);
    }

    #[test]
    fn single_wide_paragraph_does_not_invent_column() {
        let paras = vec![(36.0, 100.0, 576.0, 700.0)];
        assert!(derive_columns(&paras, 612.0, 792.0).is_empty());
    }

    #[test]
    fn stacked_chars_become_one_paragraph() {
        let mut chars = Vec::new();
        for row in 0..8 {
            let y0 = 400.0 - (row as f32) * 14.0;
            for col in 0..20 {
                let x0 = 50.0 + (col as f32) * 8.0;
                chars.push((x0, y0, x0 + 7.0, y0 + 12.0));
            }
        }
        let paras = cluster_paragraphs(&chars, 612.0, 792.0);
        assert_eq!(paras.len(), 1);
        assert!((paras[0].2 - paras[0].0) > 100.0);
    }
}
