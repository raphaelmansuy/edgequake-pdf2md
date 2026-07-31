//! SPEC-095: short cache + PDFIUM_NO_AUTO_DOWNLOAD → error (no network).

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use pdfium_auto::ensure_pdfium_library;

fn unique_cache() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("pdfium-auto-nodl-{nanos}"))
}

#[test]
fn download_min_size_rejects_short_without_network() {
    let base = unique_cache();
    let _ = std::fs::remove_dir_all(&base);
    std::env::set_var("PDFIUM_AUTO_CACHE_DIR", &base);
    std::env::remove_var("PDFIUM_LIB_PATH");
    std::env::set_var("PDFIUM_NO_AUTO_DOWNLOAD", "1");

    let cache = pdfium_auto::pdfium_cache_dir();
    std::fs::create_dir_all(&cache).unwrap();
    for name in ["libpdfium.dylib", "libpdfium.so", "pdfium.dll"] {
        std::fs::write(cache.join(name), [0u8; 100]).unwrap();
    }

    let err = ensure_pdfium_library(None).expect_err("short cache must not pass as hit");
    let msg = err.to_string();
    assert!(
        msg.contains("PDFIUM_NO_AUTO_DOWNLOAD") || msg.contains("auto-download disabled"),
        "expected NO_AUTO_DOWNLOAD error, got: {msg}"
    );

    std::env::remove_var("PDFIUM_NO_AUTO_DOWNLOAD");
    std::env::remove_var("PDFIUM_AUTO_CACHE_DIR");
    let _ = std::fs::remove_dir_all(&base);
}
