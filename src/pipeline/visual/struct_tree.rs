//! L0 StructTree region proposer (SPEC-049 / ISO §14.7).
//!
//! Uses public pdfium-render bindings (`get_handle_from_page` + `FPDF_StructTree_*`)
//! — no reliance on crate-private `page_handle`. Proposals require a resolvable
//! bbox (Layout `/BBox` attribute and/or MCID → page-object bounds). Captions
//! (`/Alt`) label only; they never invent geometry.

use std::collections::HashSet;
use std::os::raw::{c_int, c_ulong, c_void};
use std::ptr;

use pdfium_render::prelude::*;
use tracing::debug;

use super::geometry::{area_ok, pad_bbox, MAX_AREA_FRAC};
use super::types::{BBox, RegionKind, RegionProposal, RegionSource};

/// PDFium object type for attribute arrays (Layout `/BBox`).
const FPDF_OBJECT_ARRAY: i32 = 5;
/// PDFium page object type: Image.
const FPDF_PAGEOBJ_IMAGE: i32 = 3;
/// PDFium page object type: Form XObject.
const FPDF_PAGEOBJ_FORM: i32 = 5;

/// Proposes regions from the page StructTree (ISO §14.7).
pub trait StructTreeProposer: Send + Sync {
    fn propose(&self, page: &PdfPage<'_>, page_num: usize) -> Vec<RegionProposal>;
}

/// Default: walk Pdfium StructTree for `Figure` / `Table` with resolvable geometry.
#[derive(Debug, Default, Clone, Copy)]
pub struct PdfiumStructTreeProposer;

impl StructTreeProposer for PdfiumStructTreeProposer {
    fn propose(&self, page: &PdfPage<'_>, page_num: usize) -> Vec<RegionProposal> {
        propose_from_struct_tree(page, page_num)
    }
}

/// Empty proposer for L1-only tests / forced fallback.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnavailableStructTreeProposer;

impl StructTreeProposer for UnavailableStructTreeProposer {
    fn propose(&self, _page: &PdfPage<'_>, _page_num: usize) -> Vec<RegionProposal> {
        Vec::new()
    }
}

/// True when Pdfium returns a non-null structure tree for the page (telemetry).
pub fn page_has_struct_tree(page: &PdfPage<'_>) -> bool {
    let bindings = page.bindings();
    let page_handle = bindings.get_handle_from_page(page);
    let tree = bindings.FPDF_StructTree_GetForPage(page_handle);
    if tree.is_null() {
        return false;
    }
    bindings.FPDF_StructTree_Close(tree);
    true
}

fn propose_from_struct_tree(page: &PdfPage<'_>, page_num: usize) -> Vec<RegionProposal> {
    let bindings = page.bindings();
    let page_handle = bindings.get_handle_from_page(page);
    let tree = bindings.FPDF_StructTree_GetForPage(page_handle);
    if tree.is_null() {
        return Vec::new();
    }

    let page_width = page.width().value.max(1.0);
    let page_height = page.height().value.max(1.0);
    let page_area = page_width * page_height;
    let mcid_index = build_mcid_object_index(bindings, page_handle);

    let mut out = Vec::new();
    let root_count = bindings.FPDF_StructTree_CountChildren(tree);
    for i in 0..root_count.max(0) {
        let child = bindings.FPDF_StructTree_GetChildAtIndex(tree, i);
        if child.is_null() {
            continue;
        }
        walk_struct_element(
            bindings,
            child,
            page_num,
            page_width,
            page_height,
            page_area,
            &mcid_index,
            &mut out,
        );
    }

    bindings.FPDF_StructTree_Close(tree);
    debug!(
        page_num,
        l0 = out.len(),
        "StructTree L0 proposals (SPEC-049)"
    );
    out
}

#[allow(clippy::too_many_arguments)]
fn walk_struct_element(
    bindings: &dyn PdfiumLibraryBindings,
    element: FPDF_STRUCTELEMENT,
    page_num: usize,
    page_width: f32,
    page_height: f32,
    page_area: f32,
    mcid_index: &McidObjectIndex,
    out: &mut Vec<RegionProposal>,
) {
    let type_name = struct_element_type(bindings, element);
    let is_figure = type_name.eq_ignore_ascii_case("Figure");
    let is_table = type_name.eq_ignore_ascii_case("Table");

    if is_figure || is_table {
        if let Some(prop) = proposal_from_element(
            bindings,
            element,
            page_num,
            if is_table {
                RegionKind::Table
            } else {
                RegionKind::Figure
            },
            page_width,
            page_height,
            page_area,
            mcid_index,
        ) {
            out.push(prop);
        }
    }

    let child_count = bindings.FPDF_StructElement_CountChildren(element);
    for i in 0..child_count.max(0) {
        let child = bindings.FPDF_StructElement_GetChildAtIndex(element, i);
        if child.is_null() {
            continue;
        }
        walk_struct_element(
            bindings,
            child,
            page_num,
            page_width,
            page_height,
            page_area,
            mcid_index,
            out,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn proposal_from_element(
    bindings: &dyn PdfiumLibraryBindings,
    element: FPDF_STRUCTELEMENT,
    page_num: usize,
    kind: RegionKind,
    page_width: f32,
    page_height: f32,
    page_area: f32,
    mcid_index: &McidObjectIndex,
) -> Option<RegionProposal> {
    let mcids = collect_element_mcids(bindings, element);
    let (mut bbox, has_image, has_form) = bbox_from_mcids(mcid_index, &mcids)
        .map(|(b, img, form)| (Some(b), img, form))
        .unwrap_or((None, false, false));

    if bbox.is_none() {
        bbox = attr_layout_bbox(bindings, element);
    }

    let bbox = bbox?;
    let padded = pad_bbox(bbox, page_width, page_height);
    if !l0_area_ok(padded, page_area, has_image) {
        return None;
    }

    let label = struct_element_alt_text(bindings, element)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| match kind {
            RegionKind::Figure => "Figure".into(),
            RegionKind::Table => "Table".into(),
        });

    Some(RegionProposal {
        page_num,
        kind,
        source: RegionSource::StructTree,
        bbox: padded,
        label,
        has_image,
        has_form,
        has_ruled_paths: false,
    })
}

fn l0_area_ok(bbox: BBox, page_area: f32, has_image: bool) -> bool {
    let w = (bbox.2 - bbox.0).abs();
    let h = (bbox.3 - bbox.1).abs();
    let frac = (w * h) / page_area.max(1.0);
    if frac > MAX_AREA_FRAC {
        return false;
    }
    if has_image {
        return w >= 24.0 && h >= 24.0;
    }
    area_ok(bbox, page_area)
}

fn collect_element_mcids(
    bindings: &dyn PdfiumLibraryBindings,
    element: FPDF_STRUCTELEMENT,
) -> Vec<i32> {
    let mut ids = Vec::new();
    let count = bindings.FPDF_StructElement_GetMarkedContentIdCount(element);
    if count > 0 {
        for i in 0..count {
            let id = bindings.FPDF_StructElement_GetMarkedContentIdAtIndex(element, i);
            if id >= 0 {
                ids.push(id);
            }
        }
    } else {
        let id = bindings.FPDF_StructElement_GetMarkedContentID(element);
        if id >= 0 {
            ids.push(id);
        }
    }

    let child_count = bindings.FPDF_StructElement_CountChildren(element);
    for i in 0..child_count.max(0) {
        let kid_mcid = bindings.FPDF_StructElement_GetChildMarkedContentID(element, i);
        if kid_mcid >= 0 {
            ids.push(kid_mcid);
        }
    }

    ids.sort_unstable();
    ids.dedup();
    ids
}

struct McidObjectIndex {
    /// MCID → (bbox, is_image, is_form)
    entries: Vec<(i32, BBox, bool, bool)>,
}

fn build_mcid_object_index(
    bindings: &dyn PdfiumLibraryBindings,
    page_handle: FPDF_PAGE,
) -> McidObjectIndex {
    let mut entries = Vec::new();
    let n = bindings.FPDFPage_CountObjects(page_handle);
    for i in 0..n.max(0) {
        let obj = bindings.FPDFPage_GetObject(page_handle, i);
        if obj.is_null() {
            continue;
        }
        let mcid = bindings.FPDFPageObj_GetMarkedContentID(obj);
        if mcid < 0 {
            continue;
        }
        let Some(bbox) = page_object_bbox(bindings, obj) else {
            continue;
        };
        let obj_type = bindings.FPDFPageObj_GetType(obj);
        let is_image = obj_type == FPDF_PAGEOBJ_IMAGE;
        let is_form = obj_type == FPDF_PAGEOBJ_FORM;
        entries.push((mcid, bbox, is_image, is_form));
    }
    McidObjectIndex { entries }
}

fn bbox_from_mcids(index: &McidObjectIndex, mcids: &[i32]) -> Option<(BBox, bool, bool)> {
    if mcids.is_empty() {
        return None;
    }
    let wanted: HashSet<i32> = mcids.iter().copied().collect();
    let mut union: Option<BBox> = None;
    let mut has_image = false;
    let mut has_form = false;
    for (mcid, bbox, img, form) in &index.entries {
        if !wanted.contains(mcid) {
            continue;
        }
        has_image |= *img;
        has_form |= *form;
        union = Some(match union {
            None => *bbox,
            Some(u) => (
                u.0.min(bbox.0),
                u.1.min(bbox.1),
                u.2.max(bbox.2),
                u.3.max(bbox.3),
            ),
        });
    }
    union.map(|b| (b, has_image, has_form))
}

fn page_object_bbox(bindings: &dyn PdfiumLibraryBindings, obj: FPDF_PAGEOBJECT) -> Option<BBox> {
    let mut left = 0.0_f32;
    let mut bottom = 0.0_f32;
    let mut right = 0.0_f32;
    let mut top = 0.0_f32;
    if !bindings.is_true(bindings.FPDFPageObj_GetBounds(
        obj,
        &mut left,
        &mut bottom,
        &mut right,
        &mut top,
    )) {
        return None;
    }
    let w = (right - left).abs();
    let h = (top - bottom).abs();
    if w < 1.0 && h < 1.0 {
        return None;
    }
    Some((left, bottom, right, top))
}

fn attr_layout_bbox(
    bindings: &dyn PdfiumLibraryBindings,
    element: FPDF_STRUCTELEMENT,
) -> Option<BBox> {
    let attr_count = bindings.FPDF_StructElement_GetAttributeCount(element);
    for i in 0..attr_count.max(0) {
        let attr = bindings.FPDF_StructElement_GetAttributeAtIndex(element, i);
        if attr.is_null() {
            continue;
        }
        let value = bindings.FPDF_StructElement_Attr_GetValue(attr, "BBox");
        if value.is_null() {
            continue;
        }
        if let Some(bbox) = attr_value_as_bbox(bindings, value) {
            return Some(bbox);
        }
    }
    None
}

fn attr_value_as_bbox(
    bindings: &dyn PdfiumLibraryBindings,
    value: FPDF_STRUCTELEMENT_ATTR_VALUE,
) -> Option<BBox> {
    let ty = bindings.FPDF_StructElement_Attr_GetType(value);
    if ty != FPDF_OBJECT_ARRAY {
        return None;
    }
    let n = bindings.FPDF_StructElement_Attr_CountChildren(value);
    if n != 4 {
        return None;
    }
    let mut nums = [0.0_f32; 4];
    for (j, slot) in nums.iter_mut().enumerate() {
        let child = bindings.FPDF_StructElement_Attr_GetChildAtIndex(value, j as c_int);
        if child.is_null() {
            return None;
        }
        let mut v = 0.0_f32;
        if !bindings.is_true(bindings.FPDF_StructElement_Attr_GetNumberValue(child, &mut v)) {
            return None;
        }
        *slot = v;
    }
    Some((nums[0], nums[1], nums[2], nums[3]))
}

fn struct_element_type(
    bindings: &dyn PdfiumLibraryBindings,
    element: FPDF_STRUCTELEMENT,
) -> String {
    read_utf16le_api(|buf, len| bindings.FPDF_StructElement_GetType(element, buf, len))
}

fn struct_element_alt_text(
    bindings: &dyn PdfiumLibraryBindings,
    element: FPDF_STRUCTELEMENT,
) -> Option<String> {
    let s = read_utf16le_api(|buf, len| bindings.FPDF_StructElement_GetAltText(element, buf, len));
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn read_utf16le_api(mut get: impl FnMut(*mut c_void, c_ulong) -> c_ulong) -> String {
    let len = get(ptr::null_mut(), 0);
    if len == 0 {
        return String::new();
    }
    let mut buf = vec![0u8; len as usize];
    get(buf.as_mut_ptr() as *mut c_void, len);
    utf16le_nul_string(&buf)
}

fn utf16le_nul_string(buf: &[u8]) -> String {
    let u16s: Vec<u16> = buf
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let end = u16s.iter().position(|&c| c == 0).unwrap_or(u16s.len());
    String::from_utf16_lossy(&u16s[..end])
}

#[cfg(test)]
pub mod test_support {
    use super::*;

    /// Injects fixed L0 proposals for cascade unit tests.
    pub struct FakeStructTreeProposer {
        pub proposals: Vec<RegionProposal>,
    }

    impl StructTreeProposer for FakeStructTreeProposer {
        fn propose(&self, _page: &PdfPage<'_>, page_num: usize) -> Vec<RegionProposal> {
            self.proposals
                .iter()
                .filter(|p| p.page_num == page_num)
                .cloned()
                .map(|mut p| {
                    p.source = RegionSource::StructTree;
                    p
                })
                .collect()
        }
    }

    pub fn sample_l0_figure(page_num: usize) -> RegionProposal {
        RegionProposal {
            page_num,
            kind: RegionKind::Figure,
            source: RegionSource::StructTree,
            bbox: (100.0, 200.0, 400.0, 500.0),
            label: "Figure 1".into(),
            has_image: false,
            has_form: true,
            has_ruled_paths: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn utf16_helper_strips_nul() {
        // 'A' 'B' NUL
        let buf = [b'A', 0, b'B', 0, 0, 0];
        assert_eq!(utf16le_nul_string(&buf), "AB");
    }

    #[test]
    fn tagged_figure_sample_yields_l0_struct_tree() {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_cases/tagged_figure_sample.pdf");
        if !path.exists() {
            eprintln!("skip: missing {path:?}");
            return;
        }
        let pdfium = match crate::pipeline::render::get_pdfium() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("skip pdfium: {e}");
                return;
            }
        };
        let doc = match pdfium.load_pdf_from_file(&path, None) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("skip open: {e}");
                return;
            }
        };
        let page = doc.pages().get(0).expect("page 0");
        assert!(
            page_has_struct_tree(&page),
            "fixture must be Tagged with StructTree"
        );
        let props = PdfiumStructTreeProposer.propose(&page, 1);
        assert!(
            !props.is_empty(),
            "expected L0 Figure from tagged fixture, got none"
        );
        assert!(props.iter().all(|p| p.source == RegionSource::StructTree));
        assert!(props.iter().any(|p| p.kind == RegionKind::Figure));
        assert!(
            props.iter().any(|p| p.label.contains("Figure")),
            "labels={:?}",
            props.iter().map(|p| &p.label).collect::<Vec<_>>()
        );
    }
}
