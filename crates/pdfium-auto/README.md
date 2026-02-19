# pdfium-auto

> Auto-download and cache [PDFium](https://pdfium.googlesource.com/pdfium/)
> binaries at runtime — zero-friction setup for
> [pdfium-render](https://crates.io/crates/pdfium-render).

## Problem

`pdfium-render` is excellent, but requires users to manually:

1. Run a setup script to download 30 MB of platform-specific binaries
2. Export `DYLD_LIBRARY_PATH` / `LD_LIBRARY_PATH` before running

This blocks `cargo install` workflows: users cannot simply
`cargo install edgequake-pdf2md && pdf2md document.pdf`.

## Solution

`pdfium-auto` wraps `pdfium-render` with automatic library management:

| Step | Before | After |
|------|--------|-------|
| Install | `cargo install && ./scripts/setup-pdfium.sh` | `cargo install` |
| Setup | `export DYLD_LIBRARY_PATH=$(pwd)` | *(nothing)* |
| First run | Error: cannot find `libpdfium.dylib` | Download → cache → run |
| Subsequent runs | Needs env var | Instant start from cache |

## Usage

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

## Cache Locations

| Platform | Default cache path |
|----------|--------------------|
| macOS    | `~/Library/Caches/pdf2md/pdfium-{VERSION}/` |
| Linux    | `~/.cache/pdf2md/pdfium-{VERSION}/` |
| Windows  | `%LOCALAPPDATA%\pdf2md\pdfium-{VERSION}\` |

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `PDFIUM_LIB_PATH` | Full path to existing pdfium library; skips download |
| `PDFIUM_AUTO_CACHE_DIR` | Override the base cache directory |

## Platform Support

| OS | Arch | Library |
|----|------|---------|
| macOS | arm64 (Apple Silicon) | `libpdfium.dylib` |
| macOS | x86_64 (Intel) | `libpdfium.dylib` |
| Linux | x86_64 | `libpdfium.so` |
| Linux | aarch64 | `libpdfium.so` |
| Windows | x86_64 | `pdfium.dll` |
| Windows | aarch64 | `pdfium.dll` |

## License

MIT OR Apache-2.0
