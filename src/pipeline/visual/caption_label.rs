//! Caption geometry labeler — attaches labels; never invents regions (SPEC-049).

use pdfium_render::prelude::*;

use super::types::{BBox, RegionKind, RegionProposal};

/// Attach `Figure N` / `Table N` labels when caption text lies near the proposal.
pub fn attach_caption_labels(page: &PdfPage<'_>, proposals: &mut [RegionProposal]) {
    if proposals.is_empty() {
        return;
    }
    let Ok(text) = page.text() else {
        return;
    };
    let opts = PdfSearchOptions::new();
    let fig_hits = caption_hits(&text, "Figure ", &opts);
    let fig_hits_b = caption_hits(&text, "Fig. ", &opts);
    let table_hits = caption_hits(&text, "Table ", &opts);

    for p in proposals.iter_mut() {
        let fig_all: Vec<&CaptionHit> = fig_hits.iter().chain(fig_hits_b.iter()).collect();
        let candidate_hits: Vec<&CaptionHit> = match p.kind {
            RegionKind::Figure => fig_all,
            RegionKind::Table => table_hits.iter().collect(),
        };
        let mut best: Option<&CaptionHit> = None;
        let mut best_dist = f32::MAX;
        for hit in candidate_hits {
            let d = vertical_distance(p.bbox, hit.bbox);
            if d < best_dist && d < 80.0 {
                best_dist = d;
                best = Some(hit);
            }
        }
        if let Some(hit) = best {
            p.label = hit.label.clone();
            // Refine kind from caption when cluster was ambiguous.
            if hit.label.starts_with("Table") {
                p.kind = RegionKind::Table;
            } else if hit.label.starts_with("Figure") {
                p.kind = RegionKind::Figure;
            }
        }
    }
}

#[derive(Debug, Clone)]
struct CaptionHit {
    label: String,
    bbox: BBox,
}

fn caption_hits(text: &PdfPageText<'_>, needle: &str, opts: &PdfSearchOptions) -> Vec<CaptionHit> {
    let Ok(search) = text.search(needle, opts) else {
        return Vec::new();
    };
    let mut hits = Vec::new();
    for segments in search.iter(PdfSearchDirection::SearchForward) {
        let Ok(seg) = segments.get(0) else {
            continue;
        };
        let rect = seg.bounds();
        let left = rect.left().value;
        let bottom = rect.bottom().value;
        let right = rect.right().value;
        let top = rect.top().value;
        let line = text.inside_rect(PdfRect::new(
            PdfPoints::new(bottom - 2.0),
            PdfPoints::new(left),
            PdfPoints::new(top + 2.0),
            PdfPoints::new(left + 280.0),
        ));
        let window = if line.trim().is_empty() {
            seg.text()
        } else {
            line
        };
        let Some(num) = parse_caption_number(&window, needle) else {
            continue;
        };
        let kind = if needle.starts_with("Fig") {
            "Figure"
        } else {
            "Table"
        };
        hits.push(CaptionHit {
            label: format!("{kind} {num}"),
            bbox: (left, bottom, right, top),
        });
    }
    // Prefer unique labels (first occurrence).
    let mut seen = std::collections::HashSet::new();
    hits.retain(|h| seen.insert(h.label.clone()));
    hits
}

fn parse_caption_number(window: &str, needle: &str) -> Option<u32> {
    let lower_needle = needle.to_ascii_lowercase();
    let lower = window.to_ascii_lowercase();
    let start = lower.find(&lower_needle).unwrap_or(0);
    let rest = window.get(start + needle.len()..).unwrap_or("");
    let rest = rest.trim_start_matches(|c: char| c == '.' || c.is_whitespace());
    let mut digits = String::new();
    for c in rest.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else if !digits.is_empty() {
            break;
        } else if !c.is_whitespace() && c != '.' {
            return None;
        }
    }
    digits.parse().ok()
}

fn vertical_distance(visual: BBox, caption: BBox) -> f32 {
    let v_mid_x = (visual.0 + visual.2) * 0.5;
    let c_mid_x = (caption.0 + caption.2) * 0.5;
    let dx = (v_mid_x - c_mid_x).abs();
    // Caption typically just below visual (PDF y-up → caption.bottom < visual.bottom).
    let dy = if caption.3 <= visual.1 + 4.0 {
        (visual.1 - caption.3).abs()
    } else if caption.1 >= visual.3 - 4.0 {
        (caption.1 - visual.3).abs()
    } else {
        // Horizontal overlap with vertical overlap — treat as close.
        0.0
    };
    dy + dx * 0.25
}

#[cfg(test)]
mod tests {
    use super::parse_caption_number;

    #[test]
    fn parses_figure_numbers() {
        assert_eq!(
            parse_caption_number("Figure 1: Overview", "Figure "),
            Some(1)
        );
        assert_eq!(parse_caption_number("Fig. 12. Results", "Fig. "), Some(12));
        assert_eq!(parse_caption_number("Table 3: Rates", "Table "), Some(3));
        assert_eq!(parse_caption_number("see figure later", "Figure "), None);
    }
}
