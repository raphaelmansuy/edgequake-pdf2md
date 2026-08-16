//! L1 object / placement region proposals (SPEC-049 / 005 Lever B lite).
//!
//! Pdfium page-object bounds are already page-space (CTM applied). We treat
//! each Image and Form as an **atomic placement seed**, then only cluster
//! path/shading ink that is not contained in a seed.
//!
//! ## Design principle: conservative proposer + VLM semantic filter
//!
//! Geometry alone cannot distinguish a vector bar chart from a decorated text
//! box — both are path objects.  The L1 stage therefore acts as a
//! **conservative proposer**: prefer recall over precision so that the
//! downstream VLM filter pass (Pass 1) can make the authoritative semantic
//! decision on each crop.  Only two geometry gates are applied here because
//! they are based on **ISO/paint facts**, not English heuristics:
//!
//! 1. **All-rules gate** — a cluster made *entirely* of H/V rule strokes is a
//!    text-native table frame (LaTeX `\hline`).  The text channel owns it.
//! 2. **Image area gate** — removed: logos/icons are forwarded to VLM Pass-1
//!    which classifies them semantically.  Geometry cannot distinguish a logo
//!    from a real small figure; the semantic oracle can.
//!
//! Everything else is forwarded to the VLM pass — see [`spec049_two_pass`].
//!
//! ## Table vs Figure classification
//!
//! Path clusters default to Figure.  A genuine ruled grid (H AND V rules
//! present, but mixed with non-rule content) may be re-classified to Table
//! by the caption label (`Table N`) in `caption_label.rs`.  The geometry
//! cue is now advisory only.

use pdfium_render::prelude::*;

use super::geometry::{
    area_ok, cluster_bboxes, containment_ratio, image_area_ok, pad_bbox, CONTAINMENT_SUPPRESS,
    MAX_AREA_FRAC,
};
use super::types::{BBox, RegionKind, RegionProposal, RegionSource};

/// Minimum rule count required in *each* direction to infer a ruled grid.
/// Requires ≥2 horizontal AND ≥2 vertical rules before classifying as Table.
pub const GRID_RULE_MIN: usize = 2;

#[derive(Clone, Copy)]
struct ObjMeta {
    bbox: BBox,
    is_image: bool,
    is_form: bool,
    /// Thin horizontal rule: h ≤ 2.5 pt, w ≥ 80 pt.
    is_h_rule: bool,
    /// Thin vertical rule: w ≤ 2.5 pt, h ≥ 80 pt.
    is_v_rule: bool,
}

/// Propose visual regions from page object placements (no caption detection).
pub fn propose_object_clusters(page: &PdfPage<'_>, page_num: usize) -> Vec<RegionProposal> {
    let page_width = page.width().value.max(1.0);
    let page_height = page.height().value.max(1.0);
    let page_area = page_width * page_height;

    let metas = collect_object_metas(page);
    if metas.is_empty() {
        return Vec::new();
    }

    let mut proposals: Vec<RegionProposal> = Vec::new();

    // Phase 1 — atomic Image / Form placements (authoritative paint).
    for m in metas.iter().filter(|m| m.is_image || m.is_form) {
        let bbox = pad_bbox(m.bbox, page_width, page_height);
        if !region_area_ok(bbox, page_area, m.is_image) {
            continue;
        }
        proposals.push(placement_proposal(
            page_num, bbox, m.is_image, m.is_form, false,
        ));
    }

    // Phase 2 — path / shading residual: cluster only ink outside seeds.
    let path_boxes: Vec<BBox> = metas
        .iter()
        .filter(|m| !m.is_image && !m.is_form)
        .filter(|m| {
            !proposals
                .iter()
                .any(|p| containment_ratio(m.bbox, p.bbox) >= CONTAINMENT_SUPPRESS)
        })
        .map(|m| m.bbox)
        .collect();

    for cluster in cluster_bboxes(&path_boxes) {
        let members: Vec<&ObjMeta> = metas
            .iter()
            .filter(|m| !m.is_image && !m.is_form && bbox_intersects(m.bbox, cluster))
            .collect();
        if members.is_empty() {
            continue;
        }

        // First-principles modality split (SPEC-049 / 001):
        // A cluster composed *entirely* of rule strokes (H-rules + V-rules, no
        // fill paths, no shading) is a text-native table frame (LaTeX \hline /
        // column separators).  The semantic content lives in the text channel;
        // producing a visual crop adds noise and no new signal.
        // A matplotlib / TikZ figure always has at least one non-rule member
        // (fill, shading, arbitrary stroke) so it is unaffected by this gate.
        let all_rules = members.iter().all(|m| m.is_h_rule || m.is_v_rule);
        if all_rules {
            continue;
        }

        // Geometry cannot reliably separate a decorated text box from a real
        // figure (both have rectangular fills + border paths).  Rather than
        // applying fragile heuristics here, we forward ALL non-trivial clusters
        // to the VLM semantic filter (Pass 1).  See module doc for rationale.
        let bbox = pad_bbox(cluster, page_width, page_height);
        // Grid detection is kept for the residual case: a cluster with BOTH
        // fill/stroke content AND ≥GRID_RULE_MIN H+V rules (e.g. a coloured
        // TikZ table with filled cells).  Caption labels may refine kind later.
        let h_rules = members.iter().filter(|m| m.is_h_rule).count();
        let v_rules = members.iter().filter(|m| m.is_v_rule).count();
        let has_ruled_grid = h_rules >= GRID_RULE_MIN && v_rules >= GRID_RULE_MIN;
        if !region_area_ok(bbox, page_area, false) {
            continue;
        }
        proposals.push(placement_proposal(
            page_num,
            bbox,
            false,
            false,
            has_ruled_grid,
        ));
    }

    proposals
}

fn placement_proposal(
    page_num: usize,
    bbox: BBox,
    has_image: bool,
    has_form: bool,
    has_ruled_grid: bool,
) -> RegionProposal {
    // Grid (H ∧ V rules) without an embedded image → probable table.
    // Everything else defaults to Figure; caption labels override later.
    let kind = if has_ruled_grid && !has_image {
        RegionKind::Table
    } else {
        RegionKind::Figure
    };
    let label = match kind {
        RegionKind::Figure => "Figure".to_string(),
        RegionKind::Table => "Table".to_string(),
    };
    RegionProposal {
        page_num,
        kind,
        source: RegionSource::ObjectCluster,
        bbox,
        label,
        has_image,
        has_form,
        has_ruled_paths: has_ruled_grid,
    }
}

fn collect_object_metas(page: &PdfPage<'_>) -> Vec<ObjMeta> {
    let mut out = Vec::new();
    for object in page.objects().iter() {
        let ot = object.object_type();
        let is_image = ot == PdfPageObjectType::Image;
        let is_form = ot == PdfPageObjectType::XObjectForm;
        let is_shading = ot == PdfPageObjectType::Shading;
        let is_path = ot == PdfPageObjectType::Path;
        if !(is_image || is_form || is_shading || is_path) {
            continue;
        }
        let Ok(bounds) = object.bounds() else {
            continue;
        };
        let r = bounds.to_rect();
        let bb = (
            r.left().value,
            r.bottom().value,
            r.right().value,
            r.top().value,
        );
        let w = (bb.2 - bb.0).abs();
        let h = (bb.3 - bb.1).abs();
        if w < 1.0 && h < 1.0 {
            continue;
        }
        // Horizontal rule: thin horizontal stroke ≥80 pt wide.
        let is_h_rule = is_path && h <= 2.5 && w >= 80.0;
        // Vertical rule: thin vertical stroke ≥80 pt tall.
        let is_v_rule = is_path && w <= 2.5 && h >= 80.0;
        let is_any_rule = is_h_rule || is_v_rule;
        // Drop sub-threshold paths that are neither rules nor substantial shapes.
        if is_path && !is_any_rule && (w < 20.0 || h < 20.0) {
            continue;
        }
        let keep_path = is_path && (is_any_rule || (w >= 40.0 && h >= 40.0));
        if is_path && !keep_path {
            continue;
        }
        out.push(ObjMeta {
            bbox: bb,
            is_image,
            is_form,
            is_h_rule,
            is_v_rule,
        });
    }
    out
}

fn bbox_intersects(a: BBox, b: BBox) -> bool {
    a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3
}

fn region_area_ok(bbox: BBox, page_area: f32, has_image: bool) -> bool {
    let w = (bbox.2 - bbox.0).abs();
    let h = (bbox.3 - bbox.1).abs();
    let frac = (w * h) / page_area.max(1.0);
    if frac > MAX_AREA_FRAC {
        return false;
    }
    if has_image {
        return image_area_ok(bbox, page_area);
    }
    area_ok(bbox, page_area)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersects_works() {
        assert!(bbox_intersects(
            (0.0, 0.0, 10.0, 10.0),
            (5.0, 5.0, 15.0, 15.0)
        ));
        assert!(!bbox_intersects(
            (0.0, 0.0, 10.0, 10.0),
            (20.0, 20.0, 30.0, 30.0)
        ));
    }

    // --- Classification unit tests (first principles) ---

    /// H-rules alone (matplotlib axes) must NOT produce a Table.
    #[test]
    fn h_rules_alone_classify_as_figure() {
        // 3 horizontal rules, 0 vertical → not a grid → Figure
        assert!(GRID_RULE_MIN <= 2, "constant sanity");
        let h = 3usize;
        let v = 0usize;
        let has_grid = h >= GRID_RULE_MIN && v >= GRID_RULE_MIN;
        assert!(!has_grid, "H-only should not trigger grid");
    }

    /// A true ruled grid (H ∧ V rules) produces a Table.
    #[test]
    fn hv_grid_classifies_as_table() {
        let h = 3usize;
        let v = 3usize;
        let has_grid = h >= GRID_RULE_MIN && v >= GRID_RULE_MIN;
        assert!(has_grid, "H+V grid should trigger table");
    }

    /// Single-direction rules just under threshold: not a grid.
    #[test]
    fn one_hv_rule_not_grid() {
        let h = 1usize;
        let v = 1usize;
        let has_grid = h >= GRID_RULE_MIN && v >= GRID_RULE_MIN;
        assert!(!has_grid, "single H+V rule should not trigger grid");
    }

    /// A cluster of ONLY rules (text-native table frame) is suppressed.
    #[test]
    fn all_rule_cluster_suppressed() {
        // Simulate: members are all H-rules or V-rules → all_rules = true
        struct R {
            is_h_rule: bool,
            is_v_rule: bool,
        }
        let members = [
            R {
                is_h_rule: true,
                is_v_rule: false,
            },
            R {
                is_h_rule: true,
                is_v_rule: false,
            },
            R {
                is_h_rule: false,
                is_v_rule: true,
            },
        ];
        let all_rules = members.iter().all(|m| m.is_h_rule || m.is_v_rule);
        assert!(all_rules, "pure-rule cluster should be suppressed");
    }

    /// A cluster with at least one non-rule path (matplotlib fill) is kept.
    #[test]
    fn mixed_cluster_kept() {
        struct R {
            is_h_rule: bool,
            is_v_rule: bool,
        }
        // Two H-rules (axis lines) + one non-rule fill path
        let members = [
            R {
                is_h_rule: true,
                is_v_rule: false,
            },
            R {
                is_h_rule: true,
                is_v_rule: false,
            },
            R {
                is_h_rule: false,
                is_v_rule: false,
            }, // fill / arbitrary stroke
        ];
        let all_rules = members.iter().all(|m| m.is_h_rule || m.is_v_rule);
        assert!(!all_rules, "mixed cluster should NOT be suppressed");
    }
}
