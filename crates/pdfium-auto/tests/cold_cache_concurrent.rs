//! SPEC-095: concurrent cold-cache ensure (fresh process — OnceLock unset).
#![cfg(feature = "bundled")]

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use pdfium_auto::{
    bind_pdfium_from_path, bundled_pdfium_len, ensure_pdfium_bundled, pdfium_cache_dir,
    PDFIUM_VERSION,
};

fn unique_cache() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("pdfium-auto-cold-{nanos}"))
}

#[test]
fn cold_cache_concurrent_ensure() {
    let base = unique_cache();
    let _ = std::fs::remove_dir_all(&base);
    std::env::set_var("PDFIUM_AUTO_CACHE_DIR", &base);
    std::env::remove_var("PDFIUM_LIB_PATH");

    let handles: Vec<_> = (0..8)
        .map(|_| std::thread::spawn(|| ensure_pdfium_bundled().expect("concurrent ensure")))
        .collect();

    let mut paths = Vec::new();
    for h in handles {
        paths.push(h.join().expect("thread"));
    }

    let expected = bundled_pdfium_len() as u64;
    let first = &paths[0];
    for p in &paths {
        assert_eq!(p, first, "all callers must agree on cache path");
        assert_eq!(
            std::fs::metadata(p).expect("meta").len(),
            expected,
            "final library must be full length"
        );
    }

    bind_pdfium_from_path(first).expect("bind after concurrent ensure");

    let versioned = base.join(format!("pdfium-{PDFIUM_VERSION}"));
    assert_eq!(pdfium_cache_dir(), versioned);
    // No leftover temp publish files.
    for entry in std::fs::read_dir(&versioned).expect("read cache") {
        let entry = entry.unwrap();
        let name = entry.file_name();
        let s = name.to_string_lossy();
        assert!(!s.contains(".tmp."), "temp publish file left behind: {s}");
    }

    std::env::remove_var("PDFIUM_AUTO_CACHE_DIR");
    let _ = std::fs::remove_dir_all(&base);
}
