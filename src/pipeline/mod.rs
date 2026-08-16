//! Pipeline stages for PDF-to-Markdown conversion.
//!
//! Each submodule implements exactly one transformation step.
//! Keeping stages separate makes each independently testable and lets us
//! swap implementations (e.g. switch rendering backend) without touching
//! other stages.
//!
//! ## Data Flow
//!
//! ```text
//! input ──▶ render ──▶ encode ──▶ llm ──▶ postprocess
//! (URL/path)  (pdfium)  (base64)  (VLM)   (cleanup)
//! ```
//!
//! 1. [`input`]  — canonicalise the user-supplied path or URL to a local file
//! 2. [`render`] — rasterise selected pages; runs in `spawn_blocking` because
//!    pdfium is not async-safe
//! 3. [`extract_images`] / [`visual`] — figure-bounded ImageXObjects + SPEC-049
//!    object-cluster Form/table regions (not full-page rasters)
//! 4. [`encode`] — PNG-encode and base64-wrap each `DynamicImage` for the
//!    multimodal API request body
//! 5. [`llm`]    — drive the VLM call with retry/backoff; the only stage with
//!    network I/O
//! 6. [`postprocess`] — deterministic text-cleanup rules to fix VLM quirks
//!    (markdown fences, hallucinated images, broken tables, etc.)

pub mod encode;
pub mod extract_images;
pub mod extract_regions;
pub mod input;
pub mod llm;
pub mod page_geometry;
pub mod postprocess;
pub mod render;
pub mod visual;
