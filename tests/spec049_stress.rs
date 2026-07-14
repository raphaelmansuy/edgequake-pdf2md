//! SPEC-049 stress-test — all spec-data PDFs, no LLM.
//!
//! Extracts visual regions from every PDF under `specs/049-improve-figure-extraction/data/`,
//! saves each figure crop as a PNG, and writes a `report.json` per document.
//! Output lands in `specs/049-improve-figure-extraction/e2e/markdown/<stem>/`.
//!
//! Run (single-threaded due to pdfium global mutex):
//! ```sh
//! DYLD_LIBRARY_PATH=. cargo test --test spec049_stress -- --nocapture --test-threads=1
//! ```
//!
//! The JSON reports are consumed by the Python Mistral judge:
//! ```sh
//! python3 specs/049-improve-figure-extraction/scripts/llm_judge.py
//! ```

use edgequake_pdf2md::{extract_visual_regions_from_path, RegionKind, RegionSource, MAX_AREA_FRAC};
use image::ImageFormat;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Instant;

// ── Paths ─────────────────────────────────────────────────────────────────────

fn spec_data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("SPEC049_DATA_DIR") {
        return PathBuf::from(d);
    }
    // edgequake-pdf2md/../edgequake/specs/049-improve-figure-extraction/data
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("CARGO_MANIFEST_DIR has a parent")
        .join("edgequake/specs/049-improve-figure-extraction/data")
}

fn e2e_output_root() -> PathBuf {
    if let Ok(d) = std::env::var("SPEC049_E2E_DIR") {
        return PathBuf::from(d);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("CARGO_MANIFEST_DIR has a parent")
        .join("edgequake/specs/049-improve-figure-extraction/e2e/markdown")
}

// ── Report types (serialised to JSON for the Python judge) ───────────────────

#[derive(Serialize, Debug)]
struct RegionRecord {
    page: usize,
    index: usize,
    kind: &'static str,
    source: &'static str,
    label: String,
    bbox: [f32; 4],
    width: u32,
    height: u32,
    area_frac_px: f32,
    asset_path: String, // relative to the document output dir
}

#[derive(Serialize, Debug)]
struct PdfReport {
    pdf: String,
    total_regions: usize,
    figures: usize,
    tables: usize,
    extraction_ms: u128,
    regions: Vec<RegionRecord>,
}

// ── Extraction helpers ────────────────────────────────────────────────────────

/// Format: `p{page:02}-fig-{index:02}.png` (stable, sortable)
fn asset_filename(page: usize, kind: RegionKind, index: usize) -> String {
    let kind_tag = match kind {
        RegionKind::Figure => "fig",
        RegionKind::Table => "tbl",
    };
    format!("p{page:02}-{kind_tag}-{index:02}.png")
}

/// Extract visual regions, save PNG crops, write report.json.
/// Returns the PdfReport on success.
fn run_extraction(pdf_path: &Path, out_dir: &Path) -> Result<PdfReport, String> {
    let t0 = Instant::now();
    let regions = extract_visual_regions_from_path(pdf_path, None)
        .map_err(|e| format!("extraction failed: {e}"))?;
    let extraction_ms = t0.elapsed().as_millis();

    let assets_dir = out_dir.join("assets");
    std::fs::create_dir_all(&assets_dir).map_err(|e| format!("create assets dir: {e}"))?;

    let mut records = Vec::with_capacity(regions.len());

    for r in &regions {
        let filename = asset_filename(r.page_num, r.kind, r.index);
        let full_path = assets_dir.join(&filename);
        r.image
            .save_with_format(&full_path, ImageFormat::Png)
            .map_err(|e| format!("save {filename}: {e}"))?;

        // Pixel area as a fraction of the maximum render canvas (2000×2000).
        let px_area = (r.width * r.height) as f32;
        let canvas = 2000.0_f32 * 2000.0;
        let area_frac_px = px_area / canvas;

        records.push(RegionRecord {
            page: r.page_num,
            index: r.index,
            kind: match r.kind {
                RegionKind::Figure => "Figure",
                RegionKind::Table => "Table",
            },
            source: match r.source {
                RegionSource::StructTree => "StructTree",
                RegionSource::ObjectCluster => "ObjectCluster",
            },
            label: r.label.clone(),
            bbox: [r.bbox.0, r.bbox.1, r.bbox.2, r.bbox.3],
            width: r.width,
            height: r.height,
            area_frac_px,
            asset_path: format!("assets/{filename}"),
        });
    }

    let figs = regions
        .iter()
        .filter(|r| r.kind == RegionKind::Figure)
        .count();
    let tbls = regions
        .iter()
        .filter(|r| r.kind == RegionKind::Table)
        .count();

    let report = PdfReport {
        pdf: pdf_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        total_regions: regions.len(),
        figures: figs,
        tables: tbls,
        extraction_ms,
        regions: records,
    };

    let json = serde_json::to_string_pretty(&report).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(out_dir.join("report.json"), &json)
        .map_err(|e| format!("write report.json: {e}"))?;

    Ok(report)
}

// ── Area gate helper ──────────────────────────────────────────────────────────

fn assert_no_full_page(report: &PdfReport) {
    for r in &report.regions {
        assert!(
            r.area_frac_px <= MAX_AREA_FRAC,
            "[{}] region p{} #{} area_frac_px={:.3} > MAX_AREA_FRAC={:.2} (G3)",
            report.pdf,
            r.page,
            r.index,
            r.area_frac_px,
            MAX_AREA_FRAC,
        );
        assert!(
            r.width >= 8 && r.height >= 8,
            "[{}] degenerate region p{} #{}: {}×{}",
            report.pdf,
            r.page,
            r.index,
            r.width,
            r.height,
        );
    }
}

// ── Per-PDF stress tests ──────────────────────────────────────────────────────

macro_rules! stress_test {
    (
        $test_name:ident,
        $pdf_file:literal,
        min_figs = $min_figs:expr,
        max_tables = $max_tables:expr
    ) => {
        #[test]
        fn $test_name() {
            let pdf_path = spec_data_dir().join($pdf_file);
            if !pdf_path.exists() {
                eprintln!("skip: {} not found", pdf_path.display());
                return;
            }

            let stem = Path::new($pdf_file)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let out_dir = e2e_output_root().join(&stem);
            std::fs::create_dir_all(&out_dir).expect("create out dir");

            let report = match run_extraction(&pdf_path, &out_dir) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("skip: pdfium unavailable — {e}");
                    return;
                }
            };

            println!(
                "[{}] {} figures, {} tables, {} regions, {}ms",
                report.pdf,
                report.figures,
                report.tables,
                report.total_regions,
                report.extraction_ms,
            );
            for r in &report.regions {
                println!(
                    "  {} p{:02} #{} {}×{} area={:.2}% label={:?} src={}",
                    r.kind,
                    r.page,
                    r.index,
                    r.width,
                    r.height,
                    r.area_frac_px * 100.0,
                    r.label,
                    r.source,
                );
            }

            assert_no_full_page(&report);

            assert!(
                report.figures >= $min_figs,
                "[{}] expected ≥{} figures, got {}",
                report.pdf,
                $min_figs,
                report.figures
            );
            assert!(
                report.tables <= $max_tables,
                "[{}] expected ≤{} tables, got {}",
                report.pdf,
                $max_tables,
                report.tables
            );

            println!(
                "✓ {} → {}/{} figs/tbl — assets at {}/assets/",
                report.pdf,
                report.figures,
                report.tables,
                out_dir.display()
            );
        }
    };
}

// ─────────────────────────────────────────────────────────────────────────────
// Stress tests for all 7 corpus PDFs.
//
// max_tables = 0  → arXiv papers (text tables, no visual table crops expected).
// min_figs are conservative lower bounds; actual counts are printed for tuning.
// ─────────────────────────────────────────────────────────────────────────────

stress_test!(
    stress_ideas,
    "ideas_2607.08758v1.pdf",
    min_figs = 10,
    max_tables = 0
);
stress_test!(
    stress_lightrag,
    "lighrad_2410.05779v3.pdf",
    min_figs = 5,
    max_tables = 0
);
stress_test!(
    stress_rem,
    "rem_2607.08716v1.pdf",
    min_figs = 1,
    max_tables = 0
);
stress_test!(
    stress_claude_code,
    "claude_code_2604.14228v1.pdf",
    min_figs = 1,
    max_tables = 0
);
stress_test!(
    stress_deep,
    "deep_2604.26962v2.pdf",
    min_figs = 1,
    max_tables = 0
);
stress_test!(
    stress_hierar,
    "hierar_2607.02980v1.pdf",
    min_figs = 5,
    max_tables = 0
);
stress_test!(
    stress_hipo,
    "hipo_2607.02303v1.pdf",
    min_figs = 1,
    max_tables = 0
);

// ── Summary test: verify output directory was populated ───────────────────────

#[test]
fn stress_output_directory_populated() {
    let root = e2e_output_root();
    if !root.exists() {
        // Not a failure; individual stress tests create the dirs.
        eprintln!("skip: e2e output dir not yet created (run stress tests first)");
        return;
    }
    let entries: Vec<_> = std::fs::read_dir(&root)
        .expect("read e2e dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();

    println!("E2E output directories ({}):", root.display());
    for e in &entries {
        let report_path = e.path().join("report.json");
        let assets_dir = e.path().join("assets");
        let asset_count = std::fs::read_dir(&assets_dir)
            .map(|d| d.count())
            .unwrap_or(0);
        println!(
            "  {} — report.json={} assets={}",
            e.file_name().to_string_lossy(),
            report_path.exists(),
            asset_count
        );
        assert!(
            report_path.exists(),
            "missing report.json in {:?}",
            e.path()
        );
    }
    println!("Total documents processed: {}", entries.len());
}
