//! SPEC-095: PDFIUM_LIB_PATH skips bundled extract (cache untouched).
#![cfg(feature = "bundled")]

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use pdfium_auto::{ensure_pdfium_bundled, pdfium_cache_dir};

fn unique_base(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("pdfium-auto-{tag}-{nanos}"))
}

#[test]
fn lib_path_skips_bundled_extract() {
    let root = unique_base("libpath");
    let cache_a = root.join("cache-a");
    let cache_b = root.join("cache-b");
    let _ = std::fs::remove_dir_all(&root);

    // 1) Extract a real library into cache A.
    std::env::set_var("PDFIUM_AUTO_CACHE_DIR", &cache_a);
    std::env::remove_var("PDFIUM_LIB_PATH");
    let real = ensure_pdfium_bundled().expect("seed extract");
    assert!(real.starts_with(pdfium_cache_dir()) || real.exists());

    // 2) Pin LIB_PATH to that library; point cache at empty B.
    //    Note: RESOLVED_PATH is already set — ensure checks LIB_PATH first.
    std::env::set_var("PDFIUM_LIB_PATH", &real);
    std::env::set_var("PDFIUM_AUTO_CACHE_DIR", &cache_b);

    let returned = ensure_pdfium_bundled().expect("LIB_PATH ensure");
    assert_eq!(returned, real, "must return pinned path");

    assert!(
        !cache_b.exists()
            || std::fs::read_dir(&cache_b)
                .map(|mut d| d.next().is_none())
                .unwrap_or(true)
            || {
                // versioned subdir may exist empty or only lock — no libpdfium
                let versioned = pdfium_cache_dir();
                !versioned.exists()
                    || std::fs::read_dir(&versioned)
                        .unwrap()
                        .filter_map(|e| e.ok())
                        .all(|e| {
                            let n = e.file_name().to_string_lossy().into_owned();
                            n.ends_with(".lock") || n.contains(".tmp.")
                        })
            },
        "cache B must not receive a published library when LIB_PATH is set"
    );

    // Stronger: no libpdfium* under cache_b.
    if cache_b.exists() {
        fn walk_libs(dir: &std::path::Path) -> bool {
            let Ok(rd) = std::fs::read_dir(dir) else {
                return false;
            };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() && walk_libs(&p) {
                    return true;
                }
                let n = e.file_name().to_string_lossy().into_owned();
                if n.starts_with("libpdfium") || n == "pdfium.dll" {
                    return true;
                }
            }
            false
        }
        assert!(
            !walk_libs(&cache_b),
            "no pdfium library must be written under cache B"
        );
    }

    std::env::remove_var("PDFIUM_LIB_PATH");
    std::env::remove_var("PDFIUM_AUTO_CACHE_DIR");
    let _ = std::fs::remove_dir_all(&root);
}
