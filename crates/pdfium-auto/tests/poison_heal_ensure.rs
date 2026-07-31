//! SPEC-095: truncated cache heals via ensure_pdfium_bundled alone.
#![cfg(feature = "bundled")]

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use pdfium_auto::{
    bind_pdfium_from_path, bundled_pdfium_len, ensure_pdfium_bundled, pdfium_cache_dir,
};

fn unique_cache() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("pdfium-auto-poison-{nanos}"))
}

#[test]
fn poison_heal_via_ensure_only() {
    let base = unique_cache();
    let _ = std::fs::remove_dir_all(&base);
    std::env::set_var("PDFIUM_AUTO_CACHE_DIR", &base);
    std::env::remove_var("PDFIUM_LIB_PATH");

    let cache = pdfium_cache_dir();
    std::fs::create_dir_all(&cache).unwrap();

    // Detect platform lib name by ensuring once into a sibling then… we plant
    // a short file using common names; ensure will rewrite the correct one.
    // Plant short files for all known names so whichever platform we are on is poisoned.
    for name in ["libpdfium.dylib", "libpdfium.so", "pdfium.dll"] {
        std::fs::write(cache.join(name), [0u8; 100]).unwrap();
    }

    let path = ensure_pdfium_bundled().expect("ensure must heal truncated cache");
    let len = std::fs::metadata(&path).unwrap().len();
    assert_eq!(len, bundled_pdfium_len() as u64);
    assert!(len > 100, "healed library must not remain at poison size");
    bind_pdfium_from_path(&path).expect("bind after heal");

    std::env::remove_var("PDFIUM_AUTO_CACHE_DIR");
    let _ = std::fs::remove_dir_all(&base);
}
