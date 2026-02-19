# PDF Rendering Alternatives Study

> **Date**: February 19, 2026
> **Purpose**: Evaluate alternatives to pdfium-render to simplify CLI setup
> **Current**: pdfium-render v0.8 with manual binary downloads and DYLD_LIBRARY_PATH setup

---

## Executive Summary

The current setup using `pdfium-render` requires users to manually download PDFium binaries and set library paths, creating friction in CLI installation. Several alternatives exist that could significantly simplify setup:

**Top Recommendation**: Switch to `pdfium-bind` - provides automatic binary downloads with vendored libraries, eliminating manual setup while maintaining identical rendering quality.

**Other Options**:
- `poppler-rs`: System dependency (available via Homebrew), good quality but requires external installation
- Pure Rust renderers: Immature or non-existent for production use
- `mupdf`: Excellent quality but AGPL license conflicts

---

## Current Setup Analysis

### pdfium-render v0.8.37
- **Setup Complexity**: High
  - Requires downloading ~30MB binaries from bblanchon/pdfium-binaries
  - Must set `DYLD_LIBRARY_PATH` (macOS), `LD_LIBRARY_PATH` (Linux), or `PATH` (Windows)
  - Platform-specific setup scripts needed
- **Quality**: Excellent (Chrome-grade rendering)
- **Maintenance**: Active (84 versions, updated within 3 months)
- **License**: MIT/Apache-2.0

### Setup Friction Points
1. **Manual Downloads**: Users must run `./scripts/setup-pdfium.sh` or manual curl commands
2. **Environment Variables**: Must export library paths before running
3. **Platform Detection**: Script handles OS/arch detection but still requires user execution
4. **Distribution**: Binaries not bundled with the crate

---

## Alternative Analysis

### 1. pdfium-bind v0.1.0 ⭐ **RECOMMENDED**

**Overview**: Rust bindings for PDFium with automatic vendored binary downloads.

**Setup Complexity**: Very Low
- Automatic binary downloads during build
- `dynamic` feature: Embeds library in executable, extracts to temp at runtime
- `static` feature: Links statically at build time
- Zero user setup required

**Quality**: Identical to pdfium-render (same underlying engine)

**Pros**:
- ✅ Automatic setup - no manual intervention
- ✅ Vendored binaries - self-contained distribution
- ✅ Same rendering quality as current setup
- ✅ MIT license
- ✅ Active maintenance (updated within 2 months)

**Cons**:
- ❌ Newer crate (63 downloads, 1 version)
- ❌ May need API adaptation (different from pdfium-render)

**Migration Effort**: Medium
- API differences may require code changes
- Need to test rendering output equivalence

**Verdict**: Best alternative for setup simplification

---

### 2. poppler-rs v0.25.0

**Overview**: High-level Rust bindings for poppler-glib (GNOME PDF library).

**Setup Complexity**: Medium
- Requires system poppler installation
- macOS: `brew install poppler` (readily available)
- Linux: Usually pre-installed or via `apt install libpoppler-glib-dev`
- Windows: More complex (vcpkg or MSYS2)

**Quality**: Good but inferior to pdfium
- Handles most PDFs well
- Some rendering artifacts vs. pdfium
- Missing advanced features (transparency blending)

**Pros**:
- ✅ System package on most platforms
- ✅ GPL license (permissive for most use cases)
- ✅ Mature ecosystem (173K downloads)

**Cons**:
- ❌ Requires external dependency installation
- ❌ Lower rendering quality than pdfium
- ❌ More complex API than pdfium-render

**Migration Effort**: High
- Different API surface
- May need rendering parameter adjustments

**Verdict**: Good fallback if pdfium-bind proves unsuitable

---

### 3. mupdf v0.6.0

**Overview**: Safe Rust wrapper for MuPDF library.

**Setup Complexity**: Medium-High
- Requires MuPDF system installation
- macOS: `brew install mupdf` ✓
- Linux: `apt install libmupdf-dev`
- Similar distribution challenges to current pdfium setup

**Quality**: Excellent (comparable to pdfium)

**Pros**:
- ✅ High rendering quality
- ✅ Actively maintained (343K downloads)

**Cons**:
- ❌ AGPL-3.0 license (rejected in current crate selection)
- ❌ Similar setup complexity to current pdfium

**Verdict**: Rejected due to license conflict

---

### 4. Pure Rust Renderers

**Overview**: No production-ready pure Rust PDF renderers exist.

**Candidates Evaluated**:
- `micropdf` v0.9.1: Claims "drop-in replacement for MuPDF" but only 119 downloads, immature
- `pdf-canvas`: Generation only, not rendering
- `genpdf`: Generation only
- `lopdf`: Text extraction only

**Verdict**: Not viable for production use

---

## Implementation Recommendations

### Phase 1: Evaluate pdfium-bind
1. **Create test branch**: `git checkout -b eval-pdfium-bind`
2. **Add dependency**: `pdfium-bind = "0.1"`
3. **API compatibility check**: Compare pdfium-bind API vs. pdfium-render
4. **Rendering equivalence test**: Verify identical output for sample PDFs
5. **Performance benchmark**: Ensure no regression in rendering speed

### Phase 2: Migration (if compatible)
1. **Update Cargo.toml**: Replace pdfium-render with pdfium-bind
2. **Code changes**: Adapt to new API
3. **Remove setup scripts**: Delete `scripts/setup-pdfium.sh`
4. **Update documentation**: Remove manual setup instructions
5. **Test suite**: Full E2E testing with various PDF types

### Phase 3: Fallback to poppler-rs (if needed)
- If pdfium-bind proves unsuitable, evaluate poppler-rs
- Update installation docs to include `brew install poppler`

---

## Risk Assessment

### pdfium-bind Risks
- **Maturity**: Very new crate (2 months old)
- **Maintenance**: Single maintainer, low download count
- **API Stability**: May have breaking changes

**Mitigation**: Start with evaluation branch, have poppler-rs as fallback

### Quality Regression Risk
- poppler rendering may produce different output than pdfium
- Could affect LLM extraction quality

**Mitigation**: Comprehensive testing with diverse PDF corpus

### License/Dependency Risks
- pdfium-bind: MIT ✓
- poppler-rs: GPL (acceptable)
- mupdf: AGPL (rejected)

---

## Success Metrics

**Setup Simplification**:
- Target: Zero manual steps for users
- Current: 3-4 manual commands + environment setup
- Goal: `cargo install edgequake-pdf2md` works out-of-the-box

**Quality Maintenance**:
- No visual rendering differences
- Same LLM extraction accuracy
- Equivalent performance

**Distribution**:
- Self-contained binary with no external dependencies
- Cross-platform compatibility maintained

---

## Conclusion

`pdfium-bind` represents the optimal solution for simplifying PDF rendering setup while maintaining the high quality and compatibility of the current pdfium-based approach. Its automatic binary vendoring eliminates the manual setup friction that currently hinders CLI adoption.

If `pdfium-bind` proves unsuitable due to API differences or maintenance concerns, `poppler-rs` provides a viable fallback with system-level availability on most platforms, though with some quality trade-offs.

Pure Rust alternatives remain immature and unsuitable for production use at this time.</content>
<parameter name="filePath">/Users/raphaelmansuy/Github/03-working/edgequake-pdf2md/specs/pdf-rendering-alternatives-study.md