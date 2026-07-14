//! Form/Image-first proposal precision (SPEC-049 / 005 Lever D).
//!
//! Pure refine step: never invents regions; only suppresses path-only clusters
//! that are already explained by a Form or Image placement.

use super::geometry::{containment_ratio, iou, CONTAINMENT_SUPPRESS, DEDUP_IOU};
use super::types::RegionProposal;

/// Prefer Form/Image seeds; drop path-only proposals contained in them or
/// duplicating another seed by IoU.
pub fn refine_proposals(mut proposals: Vec<RegionProposal>) -> Vec<RegionProposal> {
    if proposals.len() <= 1 {
        return proposals;
    }

    // Stable: Form/Image first so seeds win over path unions.
    proposals.sort_by_key(|p| {
        let rank = match (p.has_image, p.has_form) {
            (true, _) => 0u8,
            (false, true) => 1,
            _ => 2,
        };
        (
            rank,
            !((p.bbox.3 * 1000.0) as i64),
            (p.bbox.0 * 1000.0) as i64,
        )
    });

    let mut kept: Vec<RegionProposal> = Vec::new();
    for p in proposals {
        let is_path_only = !p.has_image && !p.has_form;
        let duplicate = kept.iter().any(|k| iou(k.bbox, p.bbox) >= DEDUP_IOU);
        if duplicate {
            continue;
        }
        if is_path_only {
            let contained = kept.iter().any(|k| {
                (k.has_image || k.has_form)
                    && containment_ratio(p.bbox, k.bbox) >= CONTAINMENT_SUPPRESS
            });
            if contained {
                continue;
            }
        }
        kept.push(p);
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::visual::types::{RegionKind, RegionSource};

    fn prop(
        has_image: bool,
        has_form: bool,
        bbox: (f32, f32, f32, f32),
        label: &str,
    ) -> RegionProposal {
        RegionProposal {
            page_num: 1,
            kind: RegionKind::Figure,
            source: RegionSource::ObjectCluster,
            bbox,
            label: label.to_string(),
            has_image,
            has_form,
            has_ruled_paths: false,
        }
    }

    #[test]
    fn suppresses_path_inside_form() {
        let form = prop(false, true, (0.0, 0.0, 100.0, 100.0), "Figure");
        let path = prop(false, false, (10.0, 10.0, 40.0, 40.0), "Figure");
        let out = refine_proposals(vec![path, form]);
        assert_eq!(out.len(), 1);
        assert!(out[0].has_form);
    }

    #[test]
    fn keeps_disjoint_form_and_path() {
        let form = prop(false, true, (0.0, 0.0, 50.0, 50.0), "Figure 1");
        let path = prop(false, false, (200.0, 200.0, 280.0, 280.0), "Figure");
        let out = refine_proposals(vec![form, path]);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn dedups_high_iou_pair() {
        let a = prop(false, true, (0.0, 0.0, 100.0, 100.0), "Figure 1");
        let b = prop(true, false, (5.0, 5.0, 95.0, 95.0), "Figure");
        let out = refine_proposals(vec![a, b]);
        assert_eq!(out.len(), 1);
    }
}
