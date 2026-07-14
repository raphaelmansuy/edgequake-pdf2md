//! Shared visual-region types (SPEC-049).

/// Kind of visual region for durable assets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionKind {
    Figure,
    Table,
}

/// How the region was proposed (cascade layer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionSource {
    /// ISO StructTree Figure/Table (L0).
    StructTree,
    /// Overlap-clustered page objects (L1).
    ObjectCluster,
}

/// PDF-space axis-aligned box `(left, bottom, right, top)`.
pub type BBox = (f32, f32, f32, f32);

/// Pre-render proposal (no bitmap yet).
#[derive(Debug, Clone)]
pub struct RegionProposal {
    pub page_num: usize,
    pub kind: RegionKind,
    pub source: RegionSource,
    pub bbox: BBox,
    pub label: String,
    /// True when an ImageXObject contributed to the cluster.
    pub has_image: bool,
    /// True when a Form XObject contributed.
    pub has_form: bool,
    /// True when a ruled **grid** (≥GRID_RULE_MIN horizontal *and* vertical
    /// rules) was found — the authoritative geometric table cue.
    /// Thin horizontal rules alone (matplotlib axes) do **not** set this flag.
    pub has_ruled_paths: bool,
}

/// One rendered caption/object region (public API, DRY with legacy PageRegion).
#[derive(Debug, Clone)]
pub struct VisualRegion {
    pub page_num: usize,
    pub index: usize,
    pub kind: RegionKind,
    pub source: RegionSource,
    pub label: String,
    pub bbox: BBox,
    pub width: u32,
    pub height: u32,
    pub image: image::DynamicImage,
    /// ImageXObject contributed to the proposal (for embed dedup).
    pub has_image: bool,
    /// Form XObject contributed (vector figure gap-fill).
    pub has_form: bool,
}
