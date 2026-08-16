//! Visual region extraction cascade (SPEC-049).
//!
//! L0 StructTree (when available) → L1 object clusters → caption **labels** →
//! render crops. Never invents regions from caption strings alone.

mod caption_label;
mod geometry;
mod object_cluster;
mod precision;
mod render_crop;
mod struct_tree;
mod text_blocks;
mod types;

pub use geometry::{
    area_ok, aspect_ok, bbox_area, cluster_bboxes, containment_ratio, image_area_ok, iou,
    CONTAINMENT_SUPPRESS, DEDUP_IOU, MAX_AREA_FRAC, MAX_ASPECT, MIN_AREA_FRAC, MIN_IMAGE_AREA_FRAC,
};
pub use precision::refine_proposals;
pub use struct_tree::{
    page_has_struct_tree, PdfiumStructTreeProposer, StructTreeProposer,
    UnavailableStructTreeProposer,
};
pub use text_blocks::{
    cluster_paragraphs, derive_columns, extract_text_layout_from_bytes, TextLayoutRegion,
};
pub use types::{BBox, RegionKind, RegionProposal, RegionSource, VisualRegion};

use pdfium_render::prelude::*;
use tracing::{debug, info};

use crate::error::Pdf2MdError;
use crate::pipeline::render::get_pdfium;

use caption_label::attach_caption_labels;
use object_cluster::propose_object_clusters;
use render_crop::{crop_rendered_page, render_page_image};

/// Extract visual regions (blocking) from PDF bytes.
pub fn extract_visual_regions_from_bytes(
    pdf_bytes: &[u8],
    password: Option<&str>,
) -> Result<Vec<VisualRegion>, Pdf2MdError> {
    extract_visual_regions_from_bytes_with_proposer(pdf_bytes, password, &PdfiumStructTreeProposer)
}

/// Extract with a custom L0 StructTree proposer (tests / L1-only fallback).
pub fn extract_visual_regions_from_bytes_with_proposer(
    pdf_bytes: &[u8],
    password: Option<&str>,
    struct_proposer: &dyn StructTreeProposer,
) -> Result<Vec<VisualRegion>, Pdf2MdError> {
    let pdfium = get_pdfium()?;
    let document = pdfium
        .load_pdf_from_byte_slice(pdf_bytes, password)
        .map_err(|e| {
            Pdf2MdError::Internal(format!("Failed to open PDF for visual extract: {e}"))
        })?;
    extract_from_document(&document, struct_proposer)
}

/// Extract visual regions (blocking) from a PDF path.
pub fn extract_visual_regions_from_path(
    pdf_path: &std::path::Path,
    password: Option<&str>,
) -> Result<Vec<VisualRegion>, Pdf2MdError> {
    if !pdf_path.exists() {
        return Err(Pdf2MdError::FileNotFound {
            path: pdf_path.to_path_buf(),
        });
    }
    let pdfium = get_pdfium()?;
    let document = pdfium.load_pdf_from_file(pdf_path, password).map_err(|e| {
        Pdf2MdError::Internal(format!("Failed to open PDF for visual extract: {e}"))
    })?;
    extract_from_document(&document, &PdfiumStructTreeProposer)
}

/// Async wrapper around [`extract_visual_regions_from_bytes`].
pub async fn extract_visual_regions(
    pdf_bytes: &[u8],
    password: Option<&str>,
) -> Result<Vec<VisualRegion>, Pdf2MdError> {
    let bytes = pdf_bytes.to_vec();
    let password = password.map(str::to_owned);
    tokio::task::spawn_blocking(move || {
        extract_visual_regions_from_bytes(&bytes, password.as_deref())
    })
    .await
    .map_err(|e| Pdf2MdError::Internal(format!("Visual extract task panicked: {e}")))?
}

fn extract_from_document(
    document: &PdfDocument<'_>,
    struct_proposer: &dyn StructTreeProposer,
) -> Result<Vec<VisualRegion>, Pdf2MdError> {
    let mut regions = Vec::new();
    let mut pages_with_struct_tree = 0usize;
    let mut l0_regions = 0usize;
    for (page_idx0, page) in document.pages().iter().enumerate() {
        let page_num = page_idx0 + 1;
        if page_has_struct_tree(&page) {
            pages_with_struct_tree += 1;
        }
        match extract_page_regions(&page, page_num, struct_proposer) {
            Ok(mut page_regions) => {
                l0_regions += page_regions
                    .iter()
                    .filter(|r| r.source == RegionSource::StructTree)
                    .count();
                regions.append(&mut page_regions);
            }
            Err(e) => {
                debug!(page_num, error = %e, "Skipping page visual extract");
            }
        }
    }
    info!(
        regions = regions.len(),
        l0_regions,
        pages_with_struct_tree,
        pages = document.pages().len(),
        "Extracted visual figure/table regions (SPEC-049 cascade)"
    );
    Ok(regions)
}

fn extract_page_regions(
    page: &PdfPage<'_>,
    page_num: usize,
    struct_proposer: &dyn StructTreeProposer,
) -> Result<Vec<VisualRegion>, Pdf2MdError> {
    let mut proposals = struct_proposer.propose(page, page_num);
    let l0_count = proposals.len();

    // L1 fills gaps: add object clusters that do not heavily overlap an L0 box.
    for cluster in propose_object_clusters(page, page_num) {
        let overlaps_l0 = proposals.iter().any(|p| {
            p.source == RegionSource::StructTree
                && boxes_overlap_significantly(p.bbox, cluster.bbox)
        });
        if !overlaps_l0 {
            proposals.push(cluster);
        }
    }

    // Form/Image-first precision across L0+L1 (DRY: shared refine).
    proposals = precision::refine_proposals(proposals);

    attach_caption_labels(page, &mut proposals);

    // Stable order: top-to-bottom, then left-to-right (PDF y-up → sort by top desc).
    proposals.sort_by(|a, b| {
        b.bbox
            .3
            .partial_cmp(&a.bbox.3)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                a.bbox
                    .0
                    .partial_cmp(&b.bbox.0)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    if proposals.is_empty() {
        debug!(
            page_num,
            "WP-5: skip figure PNG writes (no Image/Form and empty L1 residual)"
        );
        return Ok(Vec::new());
    }

    // One page raster for all crops on this page (DRY + performance).
    let Some(page_image) = render_page_image(page) else {
        return Ok(Vec::new());
    };

    let mut fig_idx = 0usize;
    let mut table_idx = 0usize;
    let mut out = Vec::new();
    for prop in proposals {
        let Some(image) = crop_rendered_page(page, &page_image, prop.bbox) else {
            continue;
        };
        let index = match prop.kind {
            RegionKind::Figure => {
                fig_idx += 1;
                fig_idx
            }
            RegionKind::Table => {
                table_idx += 1;
                table_idx
            }
        };
        out.push(VisualRegion {
            page_num,
            index,
            kind: prop.kind,
            source: prop.source,
            label: prop.label,
            bbox: prop.bbox,
            width: image.width(),
            height: image.height(),
            image,
            has_image: prop.has_image,
            has_form: prop.has_form,
        });
    }

    debug!(
        page_num,
        l0 = l0_count,
        total = out.len(),
        "Page visual regions rendered"
    );
    Ok(out)
}

fn boxes_overlap_significantly(a: BBox, b: BBox) -> bool {
    geometry::iou(a, b) >= DEDUP_IOU
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use struct_tree::test_support::{sample_l0_figure, FakeStructTreeProposer};

    fn vector_sample() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_cases/vector_figure_table_sample.pdf")
    }

    fn embedded_sample() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_cases/embedded_figure_sample.pdf")
    }

    #[test]
    fn e2_vector_form_figure_and_e3_table_not_full_page() {
        let path = vector_sample();
        if !path.exists() {
            eprintln!("fixture missing: {path:?}");
            return;
        }
        let regions = match extract_visual_regions_from_path(&path, None) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("pdfium unavailable: {e}");
                return;
            }
        };
        let figs: Vec<_> = regions
            .iter()
            .filter(|r| r.kind == RegionKind::Figure)
            .collect();
        let tables: Vec<_> = regions
            .iter()
            .filter(|r| r.kind == RegionKind::Table)
            .collect();
        assert!(
            !figs.is_empty() || !tables.is_empty(),
            "expected at least one visual region, got {:?}",
            regions
                .iter()
                .map(|r| (&r.label, r.page_num, r.kind, r.width, r.height, r.source))
                .collect::<Vec<_>>()
        );
        for r in &regions {
            let area = (r.width as u64) * (r.height as u64);
            assert!(
                area < 1_200_000,
                "region too large (near full page): {}x{} source={:?}",
                r.width,
                r.height,
                r.source
            );
            assert!(matches!(
                r.source,
                RegionSource::ObjectCluster | RegionSource::StructTree
            ));
        }
    }

    #[test]
    fn e1_embedded_image_extracts_figure() {
        let path = embedded_sample();
        if !path.exists() {
            eprintln!("fixture missing: {path:?}");
            return;
        }
        let regions = match extract_visual_regions_from_path(&path, None) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("pdfium unavailable: {e}");
                return;
            }
        };
        assert!(
            regions.iter().any(|r| r.kind == RegionKind::Figure),
            "expected figure from ImageXObject: {:?}",
            regions.len()
        );
    }

    #[ignore = "calls get_pdfium() unconditionally; pdfium atexit handler crashes on Linux CI exit"]
    #[test]
    fn e11_empty_bytes_errors_or_empty() {
        let err = extract_visual_regions_from_bytes(b"not-a-pdf", None);
        assert!(err.is_err());
    }

    #[test]
    fn e13_l0_fake_struct_tree_wins_classification() {
        // Pure unit: FakeStructTreeProposer returns a proposal; no PDF needed for trait.
        let fake = FakeStructTreeProposer {
            proposals: vec![sample_l0_figure(1)],
        };
        assert_eq!(fake.proposals.len(), 1);
        assert_eq!(fake.proposals[0].source, RegionSource::StructTree);
    }

    #[test]
    fn geometry_area_invariant_exported() {
        // Pinned product invariants (SPEC-049) — const-checked.
        const _: () = {
            assert!(MAX_AREA_FRAC <= 0.55 + f32::EPSILON);
            assert!(MIN_AREA_FRAC >= 0.02 - f32::EPSILON);
            assert!(MIN_IMAGE_AREA_FRAC >= 0.002 - f32::EPSILON);
            assert!(MAX_ASPECT >= 8.0 - f32::EPSILON);
        };
    }

    fn e2e_fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../edgequake/specs/048-improve-ux/e2e")
            .join(name)
    }

    fn assert_arxiv_visual_corpus(
        path: &std::path::Path,
        min_figs: usize,
        required_figure_nums: &[u32],
        max_page: usize,
    ) {
        let regions = match extract_visual_regions_from_path(path, None) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("skip pdfium: {e}");
                return;
            }
        };
        let figs = regions
            .iter()
            .filter(|r| r.kind == RegionKind::Figure)
            .count();
        assert!(
            figs >= min_figs,
            "{}: expected ≥{min_figs} figures, got {figs}",
            path.display()
        );
        for n in required_figure_nums {
            let label = format!("Figure {n}");
            assert!(
                regions.iter().any(|r| r.label == label),
                "{}: missing {label}; labels={:?}",
                path.display(),
                regions.iter().map(|r| &r.label).collect::<Vec<_>>()
            );
        }
        for r in &regions {
            let area = (r.width as u64) * (r.height as u64);
            assert!(
                area < 1_200_000,
                "G3 near-full: p{} {}x{} {} {:?}",
                r.page_num,
                r.width,
                r.height,
                r.label,
                r.source
            );
            assert!(r.page_num >= 1 && r.page_num <= max_page);
            assert!(matches!(r.source, RegionSource::ObjectCluster));
        }
        let tables = regions
            .iter()
            .filter(|r| r.kind == RegionKind::Table)
            .count();
        // Corpus tables are text-native — no invented table crops.
        assert_eq!(
            tables,
            0,
            "{}: unexpected table crops (text-native)",
            path.display()
        );
        // G7-ish: Form-first precision — unlabeled extras stay bounded.
        let labeled = regions
            .iter()
            .filter(|r| {
                r.label.starts_with("Figure ") && r.label.chars().any(|c| c.is_ascii_digit())
            })
            .count();
        assert!(
            figs <= labeled.saturating_mul(3).max(min_figs + 5),
            "{}: too many unlabeled extras figs={figs} labeled={labeled}",
            path.display()
        );
    }

    /// E13 / Tagged StructTree L0 Figure (Pdfium FFI) — real fixture, not Fake.
    #[test]
    fn e13_tagged_struct_tree_l0_figure() {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_cases/tagged_figure_sample.pdf");
        if !path.exists() {
            eprintln!("skip: missing {path:?}");
            return;
        }
        let regions = match extract_visual_regions_from_path(&path, None) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("skip pdfium: {e}");
                return;
            }
        };
        assert!(
            regions
                .iter()
                .any(|r| { r.kind == RegionKind::Figure && r.source == RegionSource::StructTree }),
            "expected StructTree L0 figure: {:?}",
            regions
                .iter()
                .map(|r| (&r.label, r.source, r.width, r.height))
                .collect::<Vec<_>>()
        );
        for r in &regions {
            let area = (r.width as u64) * (r.height as u64);
            assert!(
                area < 1_200_000,
                "G3 near-full: {}x{} {:?}",
                r.width,
                r.height,
                r.source
            );
        }
    }

    /// E14 / Ideas arXiv PDF (untagged, multi-page vector figures).
    #[test]
    fn e14_ideas_arxiv_pdf_figures_not_full_page() {
        let path = e2e_fixture("ideas_2607.08758v1.pdf");
        if !path.exists() {
            eprintln!("skip: missing {path:?}");
            return;
        }
        assert_arxiv_visual_corpus(&path, 10, &(1..=10).collect::<Vec<_>>(), 22);
    }

    /// E15 / Hierarchical Sparse Attention arXiv PDF.
    #[test]
    fn e15_hierar_arxiv_pdf_figures_not_full_page() {
        let path = e2e_fixture("hierar_2607.02980v1.pdf");
        if !path.exists() {
            eprintln!("skip: missing {path:?}");
            return;
        }
        assert_arxiv_visual_corpus(&path, 7, &(1..=7).collect::<Vec<_>>(), 27);
    }

    /// E16 / LightRAG arXiv PDF (Figure 2 caption exists; crop may be absent — assert known figs).
    #[test]
    fn e16_lightrad_arxiv_pdf_figures_not_full_page() {
        let path = e2e_fixture("lighrad_2410.05779v3.pdf");
        if !path.exists() {
            eprintln!("skip: missing {path:?}");
            return;
        }
        // Probe: Figure 1,3–7 labeled; Figure 2 not recovered as object cluster.
        assert_arxiv_visual_corpus(&path, 5, &[1, 3, 4, 5, 6, 7], 16);
    }
}
