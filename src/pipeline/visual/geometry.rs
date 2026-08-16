//! Pure geometry helpers for visual regions (SPEC-049) — no I/O.

use super::types::BBox;

/// Minimum region area as a fraction of the page (skip tiny ornaments).
pub const MIN_AREA_FRAC: f32 = 0.02;
/// Image/Form placements may be smaller than path clusters (logos still reach VLM).
/// Letter page ≈485k pt² — 40×30 pt sample ≈0.25%; keep floor below that.
pub const MIN_IMAGE_AREA_FRAC: f32 = 0.002;
/// Maximum region area as a fraction of the page (reject near-full-page dumps).
pub const MAX_AREA_FRAC: f32 = 0.55;
/// Reject needle-thin rules / banners (max(w,h)/min(w,h)).
pub const MAX_ASPECT: f32 = 8.0;
/// Padding around the union bbox in PDF points.
pub const PAD_PTS: f32 = 6.0;
/// IoU threshold to merge overlapping object quads into one cluster.
pub const CLUSTER_IOU: f32 = 0.05;
/// Max edge gap (pts) to merge nearly adjacent objects.
pub const CLUSTER_GAP_PTS: f32 = 12.0;
/// IoU at which two proposals are the same visual (dedup / L0 wins).
pub const DEDUP_IOU: f32 = 0.25;
/// Path-only cluster suppressed when this fraction of its area lies inside a Form/Image seed.
pub const CONTAINMENT_SUPPRESS: f32 = 0.80;

pub fn union_bboxes(boxes: &[BBox]) -> BBox {
    let mut l = f32::MAX;
    let mut b = f32::MAX;
    let mut r = f32::MIN;
    let mut t = f32::MIN;
    for bb in boxes {
        l = l.min(bb.0.min(bb.2));
        b = b.min(bb.1.min(bb.3));
        r = r.max(bb.0.max(bb.2));
        t = t.max(bb.1.max(bb.3));
    }
    (l, b, r, t)
}

pub fn bbox_area(bb: BBox) -> f32 {
    let w = (bb.2 - bb.0).abs();
    let h = (bb.3 - bb.1).abs();
    w * h
}

pub fn aspect_ok(bbox: BBox) -> bool {
    let w = (bbox.2 - bbox.0).abs().max(1.0);
    let h = (bbox.3 - bbox.1).abs().max(1.0);
    (w.max(h) / w.min(h)) <= MAX_ASPECT
}

pub fn area_ok(bbox: BBox, page_area: f32) -> bool {
    let w = (bbox.2 - bbox.0).abs();
    let h = (bbox.3 - bbox.1).abs();
    let frac = bbox_area(bbox) / page_area.max(1.0);
    (MIN_AREA_FRAC..=MAX_AREA_FRAC).contains(&frac) && w >= 40.0 && h >= 40.0 && aspect_ok(bbox)
}

/// Image/Form placement gate (SPEC-128): min area 0.8% of page + aspect + 24pt.
pub fn image_area_ok(bbox: BBox, page_area: f32) -> bool {
    let w = (bbox.2 - bbox.0).abs();
    let h = (bbox.3 - bbox.1).abs();
    let frac = bbox_area(bbox) / page_area.max(1.0);
    w >= 24.0
        && h >= 24.0
        && frac >= MIN_IMAGE_AREA_FRAC
        && frac <= MAX_AREA_FRAC
        && aspect_ok(bbox)
}

pub fn pad_bbox(visual: BBox, page_w: f32, page_h: f32) -> BBox {
    let l = visual.0 - PAD_PTS;
    let b = visual.1 - PAD_PTS;
    let r = visual.2 + PAD_PTS;
    let t = visual.3 + PAD_PTS;
    (
        l.clamp(0.0, page_w),
        b.clamp(0.0, page_h),
        r.clamp(0.0, page_w),
        t.clamp(0.0, page_h),
    )
}

pub fn iou(a: BBox, b: BBox) -> f32 {
    let l = a.0.max(b.0);
    let bot = a.1.max(b.1);
    let r = a.2.min(b.2);
    let top = a.3.min(b.3);
    let iw = (r - l).max(0.0);
    let ih = (top - bot).max(0.0);
    let inter = iw * ih;
    if inter <= 0.0 {
        return 0.0;
    }
    let ua = bbox_area(a) + bbox_area(b) - inter;
    if ua <= 0.0 {
        0.0
    } else {
        inter / ua
    }
}

/// Fraction of `inner` area that intersects `outer` (0..=1).
pub fn containment_ratio(inner: BBox, outer: BBox) -> f32 {
    let area = bbox_area(inner);
    if area <= 0.0 {
        return 0.0;
    }
    let l = inner.0.max(outer.0);
    let bot = inner.1.max(outer.1);
    let r = inner.2.min(outer.2);
    let top = inner.3.min(outer.3);
    let iw = (r - l).max(0.0);
    let ih = (top - bot).max(0.0);
    (iw * ih) / area
}

/// True when boxes overlap (IoU) or are within CLUSTER_GAP_PTS of each other.
pub fn should_merge(a: BBox, b: BBox) -> bool {
    if iou(a, b) >= CLUSTER_IOU {
        return true;
    }
    let gap_x = if a.2 < b.0 {
        b.0 - a.2
    } else if b.2 < a.0 {
        a.0 - b.2
    } else {
        0.0
    };
    let gap_y = if a.3 < b.1 {
        b.1 - a.3
    } else if b.3 < a.1 {
        a.1 - b.3
    } else {
        0.0
    };
    let x_overlap = a.0 < b.2 && b.0 < a.2;
    let y_overlap = a.1 < b.3 && b.1 < a.3;
    (x_overlap && gap_y <= CLUSTER_GAP_PTS) || (y_overlap && gap_x <= CLUSTER_GAP_PTS)
}

/// Union-find overlap clustering; returns component unions.
pub fn cluster_bboxes(boxes: &[BBox]) -> Vec<BBox> {
    let n = boxes.len();
    if n == 0 {
        return Vec::new();
    }
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for i in 0..n {
        for j in (i + 1)..n {
            if should_merge(boxes[i], boxes[j]) {
                let a = find(&mut parent, i);
                let b = find(&mut parent, j);
                if a != b {
                    parent[b] = a;
                }
            }
        }
    }
    let mut groups: Vec<Vec<BBox>> = vec![Vec::new(); n];
    for (i, bb) in boxes.iter().enumerate() {
        let root = find(&mut parent, i);
        groups[root].push(*bb);
    }
    groups
        .into_iter()
        .filter(|g| !g.is_empty())
        .map(|g| union_bboxes(&g))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn area_gate_rejects_near_full() {
        let page = 100.0 * 100.0;
        assert!(!area_ok((0.0, 0.0, 90.0, 90.0), page));
        assert!(area_ok((10.0, 10.0, 50.0, 50.0), page));
        assert!(!area_ok((0.0, 0.0, 10.0, 10.0), page));
    }

    #[test]
    fn aspect_gate_rejects_banner() {
        let page = 612.0 * 792.0;
        assert!(!aspect_ok((0.0, 0.0, 400.0, 20.0)));
        assert!(aspect_ok((0.0, 0.0, 80.0, 80.0)));
        assert!(!image_area_ok((0.0, 0.0, 20.0, 20.0), page));
        assert!(image_area_ok((0.0, 0.0, 80.0, 80.0), page));
    }

    #[test]
    fn clusters_overlapping_boxes() {
        let boxes = [
            (0.0, 0.0, 40.0, 40.0),
            (30.0, 30.0, 70.0, 70.0),
            (200.0, 200.0, 240.0, 240.0),
        ];
        let clusters = cluster_bboxes(&boxes);
        assert_eq!(clusters.len(), 2);
    }

    #[test]
    fn iou_of_identical_is_one() {
        let b = (0.0, 0.0, 10.0, 10.0);
        assert!((iou(b, b) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn containment_of_subset() {
        let outer = (0.0, 0.0, 100.0, 100.0);
        let inner = (10.0, 10.0, 40.0, 40.0);
        assert!(containment_ratio(inner, outer) > 0.99);
        assert!(containment_ratio(outer, inner) < 0.2);
    }
}
