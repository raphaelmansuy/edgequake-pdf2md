# pdfium-auto

> Auto-manage [PDFium](https://pdfium.googlesource.com/pdfium/) — zero-friction
> setup for [pdfium-render](https://crates.io/crates/pdfium-render).

[![crates.io](https://img.shields.io/crates/v/pdfium-auto.svg)](https://crates.io/crates/pdfium-auto)
[![docs.rs](https://docs.rs/pdfium-auto/badge.svg)](https://docs.rs/pdfium-auto)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE)

## Overview

`pdfium-render` is excellent but requires users to manually:

1. Run a download script for 30 MB of platform-specific binaries.
2. Export `DYLD_LIBRARY_PATH` / `LD_LIBRARY_PATH` before every run.

`pdfium-auto` eliminates both steps through two complementary modes:

| Mode | Binary | Runtime dependency | How |
|------|--------|-------------------|-----|
| **bundled** *(default)* | Self-contained | None | pdfium embedded at compile time |
| **download** | Standard | Auto-download on first use | pdfium fetched to local cache |

---

## Bundled mode (default)

When the `bundled` feature is active (the default in `edgequake-pdf2md`), the
pdfium shared library is embedded inside the binary at compile time. The
resulting executable is 100% self-contained — no internet access at runtime,
no environment variables, no manual setup.

### Build-time library resolution

`build.rs` resolves the pdfium library for the target platform in this order:

1. **`PDFIUM_BUNDLE_LIB`** — point to an existing library (fastest; skips download):
   ```bash
   PDFIUM_BUNDLE_LIB=/path/to/libpdfium.dylib cargo build --release
   ```

2. **Auto-download** — if `PDFIUM_BUNDLE_LIB` is not set, `build.rs` downloads
   the correct archive from
   [bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries)
   using `curl` and caches it in:
   ```
   {$CARGO_HOME or ~/.cargo}/pdfium-bundle/{VERSION}/{TARGET_OS}-{TARGET_ARCH}/
   ```
   Override the cache root with `PDFIUM_BUILD_CACHE_DIR`.

### Supported build targets

| OS | Arch | Archive |
|----|------|---------|
| macOS | arm64 (Apple Silicon) | pdfium-mac-arm64.tgz |
| macOS | x86_64 (Intel) | pdfium-mac-x64.tgz |
| Linux | x86_64 | pdfium-linux-x64.tgz |
| Linux | aarch64 | pdfium-linux-arm64.tgz |
| Windows | x86_64 | pdfium-win-x64.tgz |
| Windows | aarch64 | pdfium-win-arm64.tgz |
| Windows | x86 | pdfium-win-x86.tgz |

### Runtime extraction

On the first call to `bind_bundled()` (or `ensure_pdfium_bundled()`), the
embedded bytes are written to the local cache directory and marked executable.
Subsequent calls reuse the cached file — disk I/O only on first use.

| Platform | Extraction path |
|----------|----------------|
| macOS | `~/Library/Caches/pdf2md/pdfium-{VERSION}/` |
| Linux | `~/.cache/pdf2md/pdfium-{VERSION}/` |
| Windows | `%LOCALAPPDATA%\pdf2md\pdfium-{VERSION}\` |

```rust
use pdfium_auto::bind_bundled;

// Extracts lib on first call; cached on subsequent calls.
let pdfium = bind_bundled().expect("PDFium unavailable");
```

---

## Download mode (runtime auto-download)

Without the `bundled` feature, pdfium is downloaded automatically on first run
and cached locally. Subsequent runs start instantly from cache.

```rust
use pdfium_auto::{bind_pdfium_silent, ensure_pdfium_library};

// One-shot: download if needed, then bind
let pdfium = bind_pdfium_silent().expect("PDFium unavailable");

// Download with progress callback
let path = ensure_pdfium_library(Some(&|downloaded, total| {
    match total {
        Some(t) => eprint!("\rDownloading PDFium: {}%", downloaded * 100 / t),
        None    => eprint!("\rDownloading PDFium: {} bytes", downloaded),
    }
})).expect("download failed");
```

### Runtime cache locations

| Platform | Default cache path |
|----------|--------------------|
| macOS    | `~/Library/Caches/pdf2md/pdfium-{VERSION}/` |
| Linux    | `~/.cache/pdf2md/pdfium-{VERSION}/` |
| Windows  | `%LOCALAPPDATA%\pdf2md\pdfium-{VERSION}\` |

### Runtime environment variables

| Variable | Purpose |
|----------|---------|
| `PDFIUM_LIB_PATH` | Full path to an existing pdfium library; skips download |
| `PDFIUM_AUTO_CACHE_DIR` | Override the base cache directory |

---

## Cargo features

| Feature | Default | Description |
|---------|---------|-------------|
| `bundled` | **yes** (in edgequake-pdf2md) | Embed pdfium in the binary at compile time |

The `bundled` feature is **not** in the default features of the `pdfium-auto`
crate itself (it's opt-in), but it is set as a default in `edgequake-pdf2md`
so that `cargo install edgequake-pdf2md` produces a self-contained binary.

To use download-only mode (no embedded library):
```toml
[dependencies]
pdfium-auto = { version = "0.3", default-features = false }
```

---

## Build environment variables

| Variable | Scope | Purpose |
|----------|-------|---------|
| `PDFIUM_BUNDLE_LIB` | Build | Path to pdfium lib to embed (skips auto-download) |
| `PDFIUM_BUILD_CACHE_DIR` | Build | Override the auto-download cache root |

---

## PDFIUM_VERSION

This release uses pdfium chromium/**7690** from
[bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries/releases/tag/chromium%2F7690).

---

## License

MIT OR Apache-2.0
