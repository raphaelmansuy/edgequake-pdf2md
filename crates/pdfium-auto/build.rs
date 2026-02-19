// build.rs — pdfium-auto
//
// Handles the optional `bundled` feature: when active, copies the platform
// pdfium shared library (pointed to by `PDFIUM_BUNDLE_LIB`) into Cargo's
// output directory and generates a tiny Rust source file that embeds the
// bytes with `include_bytes!`.
//
// This lets the crate produce a self-contained binary where the pdfium
// library is extracted from embedded bytes at first use rather than
// downloaded or manually installed by the end-user.

use std::path::PathBuf;

fn main() {
    // Rerun this script only when relevant inputs change.
    println!("cargo:rerun-if-env-changed=PDFIUM_BUNDLE_LIB");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_BUNDLED");

    // Nothing to do unless the `bundled` feature has been activated.
    if std::env::var("CARGO_FEATURE_BUNDLED").is_err() {
        return;
    }

    // ── Locate the source library ─────────────────────────────────────────
    let lib_src = match std::env::var("PDFIUM_BUNDLE_LIB") {
        Ok(p) if !p.is_empty() => PathBuf::from(p),
        _ => {
            // Friendly compile-time error rather than a cryptic linker failure.
            panic!(
                "\n\
                 ┌─────────────────────────────────────────────────────────┐\n\
                 │  pdfium-auto: `bundled` feature activated but           │\n\
                 │  `PDFIUM_BUNDLE_LIB` is not set.                        │\n\
                 │                                                         │\n\
                 │  Set it to the path of the platform pdfium shared lib:  │\n\
                 │                                                         │\n\
                 │  macOS : path/to/libpdfium.dylib                        │\n\
                 │  Linux : path/to/libpdfium.so                           │\n\
                 │  Windows: path\\to\\pdfium.dll                            │\n\
                 │                                                         │\n\
                 │  Pre-built libraries are available from:                │\n\
                 │  https://github.com/bblanchon/pdfium-binaries/releases  │\n\
                 └─────────────────────────────────────────────────────────┘\n"
            )
        }
    };

    if !lib_src.exists() {
        panic!(
            "pdfium-auto: PDFIUM_BUNDLE_LIB points to a file that does not exist: {}",
            lib_src.display()
        );
    }

    // ── Copy into OUT_DIR with a fixed, platform-neutral name ─────────────
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR not set"));
    let lib_dest = out_dir.join("bundled_pdfium_lib");

    std::fs::copy(&lib_src, &lib_dest).unwrap_or_else(|e| {
        panic!(
            "pdfium-auto: failed to copy {} → {}: {}",
            lib_src.display(),
            lib_dest.display(),
            e
        )
    });

    // ── Generate bundled.rs ───────────────────────────────────────────────
    // We generate a tiny Rust source file rather than using `include_bytes!`
    // directly in lib.rs because the path argument to `include_bytes!` must
    // be a string literal known at the macro expansion site.  Writing the
    // macro invocation into a generated file and using `include!()` is the
    // standard Cargo pattern for this.
    let bundled_rs = out_dir.join("bundled.rs");
    let code = r#"
/// The pdfium shared library embedded at compile time.
///
/// These bytes are written to the local cache directory on first use
/// (see [`super::bind_bundled`]).
pub static PDFIUM_BYTES: &[u8] = include_bytes!("bundled_pdfium_lib");
"#;
    std::fs::write(&bundled_rs, code).unwrap_or_else(|e| {
        panic!(
            "pdfium-auto: failed to write {}: {}",
            bundled_rs.display(),
            e
        )
    });

    // Inform Cargo that bundled.rs should trigger a rebuild when changed.
    println!("cargo:rerun-if-changed={}", lib_dest.display());
}
