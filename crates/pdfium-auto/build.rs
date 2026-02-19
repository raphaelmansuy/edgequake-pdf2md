// build.rs — pdfium-auto
//
// Handles the `bundled` feature: embeds the platform-specific pdfium shared
// library inside the Rust binary at compile time.
//
// Library resolution order (first match wins)
// ─────────────────────────────────────────────
//   1. `PDFIUM_BUNDLE_LIB` env var — explicit path you supply (CI / air-gapped)
//   2. Auto-download from bblanchon/pdfium-binaries using `curl`
//
// Auto-download cache
// ───────────────────
// Downloaded libs are cached at:
//   {$CARGO_HOME or ~/.cargo}/pdfium-bundle/{VERSION}/{TARGET_OS}-{TARGET_ARCH}/{lib}
//
// Override the cache root with `PDFIUM_BUILD_CACHE_DIR`.
//
// Supported targets
// ─────────────────
//   macOS  arm64 / x86_64
//   Linux  x86_64 / aarch64
//   Windows  x86_64 / aarch64 / x86

use std::path::{Path, PathBuf};

const PDFIUM_VERSION: &str = "7690";
const BASE_URL: &str = "https://github.com/bblanchon/pdfium-binaries/releases/download";

// ── Platform metadata ────────────────────────────────────────────────────────

struct PlatformBundle {
    archive_name: &'static str,
    lib_path_in_archive: &'static str,
    lib_name: &'static str,
}

fn detect_bundle_platform(os: &str, arch: &str) -> Result<PlatformBundle, String> {
    match (os, arch) {
        ("macos", "aarch64") => Ok(PlatformBundle {
            archive_name: "pdfium-mac-arm64.tgz",
            lib_path_in_archive: "lib/libpdfium.dylib",
            lib_name: "libpdfium.dylib",
        }),
        ("macos", "x86_64") => Ok(PlatformBundle {
            archive_name: "pdfium-mac-x64.tgz",
            lib_path_in_archive: "lib/libpdfium.dylib",
            lib_name: "libpdfium.dylib",
        }),
        ("linux", "x86_64") => Ok(PlatformBundle {
            archive_name: "pdfium-linux-x64.tgz",
            lib_path_in_archive: "lib/libpdfium.so",
            lib_name: "libpdfium.so",
        }),
        ("linux", "aarch64") => Ok(PlatformBundle {
            archive_name: "pdfium-linux-arm64.tgz",
            lib_path_in_archive: "lib/libpdfium.so",
            lib_name: "libpdfium.so",
        }),
        ("windows", "x86_64") => Ok(PlatformBundle {
            archive_name: "pdfium-win-x64.tgz",
            lib_path_in_archive: "bin/pdfium.dll",
            lib_name: "pdfium.dll",
        }),
        ("windows", "aarch64") => Ok(PlatformBundle {
            archive_name: "pdfium-win-arm64.tgz",
            lib_path_in_archive: "bin/pdfium.dll",
            lib_name: "pdfium.dll",
        }),
        ("windows", "x86") => Ok(PlatformBundle {
            archive_name: "pdfium-win-x86.tgz",
            lib_path_in_archive: "bin/pdfium.dll",
            lib_name: "pdfium.dll",
        }),
        (os, arch) => Err(format!(
            "pdfium-auto[bundled]: unsupported target {os}/{arch}.\n\
             Supported: macos/aarch64|x86_64, linux/x86_64|aarch64,\n\
             windows/x86_64|aarch64|x86.\n\
             Set PDFIUM_BUNDLE_LIB=/path/to/libpdfium to provide a custom library."
        )),
    }
}

// ── Cache directory ──────────────────────────────────────────────────────────

fn build_cache_dir(target_os: &str, target_arch: &str) -> PathBuf {
    if let Ok(v) = std::env::var("PDFIUM_BUILD_CACHE_DIR") {
        return PathBuf::from(v)
            .join(PDFIUM_VERSION)
            .join(format!("{target_os}-{target_arch}"));
    }

    let cargo_home = std::env::var("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .map(PathBuf::from)
                .unwrap_or_else(|_| std::env::temp_dir());
            home.join(".cargo")
        });

    cargo_home
        .join("pdfium-bundle")
        .join(PDFIUM_VERSION)
        .join(format!("{target_os}-{target_arch}"))
}

// ── Download helper ──────────────────────────────────────────────────────────

fn download_file(url: &str, dest: &Path) {
    println!(
        "cargo:warning=pdfium-auto[bundled]: downloading {} (chromium/{PDFIUM_VERSION})…",
        url.rsplit('/').next().unwrap_or(url)
    );

    let result = std::process::Command::new("curl")
        .args([
            "-L",
            "-f",
            "-s",
            "--retry",
            "3",
            "-o",
            &dest.to_string_lossy(),
            url,
        ])
        .status();

    match result {
        Ok(s) if s.success() => return,
        Ok(s) => {
            println!("cargo:warning=pdfium-auto[bundled]: curl exited {s}, trying PowerShell…")
        }
        Err(e) => println!(
            "cargo:warning=pdfium-auto[bundled]: curl unavailable ({e}), trying PowerShell…"
        ),
    }

    // PowerShell fallback (Windows without curl in PATH)
    let ps = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!(
                "Invoke-WebRequest -Uri '{url}' -OutFile '{}' -UseBasicParsing",
                dest.display()
            ),
        ])
        .status();

    if matches!(ps, Ok(s) if s.success()) {
        return;
    }

    panic!(
        "\n\
         pdfium-auto[bundled]: failed to auto-download pdfium.\n\
         Both curl and PowerShell failed.\n\n\
         Quick fix — download manually and set:\n\
           export PDFIUM_BUNDLE_LIB=/path/to/libpdfium\n\n\
         Pre-built libraries (chromium/{PDFIUM_VERSION}):\n\
           https://github.com/bblanchon/pdfium-binaries/releases"
    );
}

// ── Extraction helper ────────────────────────────────────────────────────────

fn extract_lib(tgz_path: &Path, lib_path_in_archive: &str, dest: &Path) {
    let file = std::fs::File::open(tgz_path)
        .unwrap_or_else(|e| panic!("pdfium-auto: cannot open {}: {e}", tgz_path.display()));
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);

    for entry_result in archive
        .entries()
        .expect("pdfium-auto: failed to iterate tar archive")
    {
        let mut entry = entry_result.expect("pdfium-auto: failed to read tar entry");
        let path = entry
            .path()
            .expect("pdfium-auto: invalid tar entry path")
            .to_path_buf();

        if path.to_str() == Some(lib_path_in_archive) {
            entry.unpack(dest).unwrap_or_else(|e| {
                panic!("pdfium-auto: failed to extract '{lib_path_in_archive}': {e}")
            });
            return;
        }
    }

    panic!(
        "pdfium-auto: '{lib_path_in_archive}' not found in '{}'.\n\
         The upstream archive layout may have changed.\n\
         Set PDFIUM_BUNDLE_LIB to provide the library manually.",
        tgz_path.display()
    );
}

// ── Path resolution ──────────────────────────────────────────────────────────

fn resolve_lib(target_os: &str, target_arch: &str) -> PathBuf {
    // Priority 1: explicit env var
    if let Ok(p) = std::env::var("PDFIUM_BUNDLE_LIB") {
        if !p.is_empty() {
            let path = PathBuf::from(&p);
            if !path.exists() {
                panic!(
                    "pdfium-auto: PDFIUM_BUNDLE_LIB={p} does not exist. \
                     Check the path and try again."
                );
            }
            println!("cargo:warning=pdfium-auto[bundled]: using PDFIUM_BUNDLE_LIB={p}");
            return path;
        }
    }

    // Priority 2: auto-download with persistent cache
    let bundle = detect_bundle_platform(target_os, target_arch).unwrap_or_else(|e| panic!("{e}"));

    let cache_dir = build_cache_dir(target_os, target_arch);
    let cached_lib = cache_dir.join(bundle.lib_name);

    if cached_lib.exists() {
        println!(
            "cargo:warning=pdfium-auto[bundled]: cache hit — {} for {target_os}/{target_arch}",
            bundle.lib_name
        );
        return cached_lib;
    }

    // Cache miss: download + extract
    std::fs::create_dir_all(&cache_dir).unwrap_or_else(|e| {
        panic!(
            "pdfium-auto: failed to create cache dir {}: {e}",
            cache_dir.display()
        )
    });

    let url = format!(
        "{BASE_URL}/chromium%2F{PDFIUM_VERSION}/{}",
        bundle.archive_name
    );
    let tgz_path = cache_dir.join(bundle.archive_name);

    download_file(&url, &tgz_path);
    extract_lib(&tgz_path, bundle.lib_path_in_archive, &cached_lib);

    // Remove the compressed archive — the extracted lib stays in the cache.
    let _ = std::fs::remove_file(&tgz_path);

    println!(
        "cargo:warning=pdfium-auto[bundled]: cached {} at {}",
        bundle.lib_name,
        cached_lib.display()
    );

    cached_lib
}

// ── Entry point ──────────────────────────────────────────────────────────────

fn main() {
    println!("cargo:rerun-if-env-changed=PDFIUM_BUNDLE_LIB");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_BUNDLED");
    println!("cargo:rerun-if-env-changed=PDFIUM_BUILD_CACHE_DIR");

    if std::env::var("CARGO_FEATURE_BUNDLED").is_err() {
        return; // bundled feature not active — nothing to do
    }

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();

    let lib_src = resolve_lib(&target_os, &target_arch);

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR not set"));

    // Copy into OUT_DIR under a fixed, platform-neutral filename so the
    // include_bytes! path in the generated bundled.rs is always stable.
    let lib_dest = out_dir.join("bundled_pdfium_lib");
    std::fs::copy(&lib_src, &lib_dest).unwrap_or_else(|e| {
        panic!(
            "pdfium-auto: failed to copy {} → {}: {e}",
            lib_src.display(),
            lib_dest.display()
        )
    });

    // Generate bundled.rs (include!()-ed by lib.rs).
    let bundled_rs = out_dir.join("bundled.rs");
    std::fs::write(
        &bundled_rs,
        "/// The pdfium shared library embedded at compile time.\n\
         ///\n\
         /// On first use, these bytes are extracted to the local cache\n\
         /// directory; see [`super::bind_bundled`].\n\
         pub static PDFIUM_BYTES: &[u8] = include_bytes!(\"bundled_pdfium_lib\");\n",
    )
    .unwrap_or_else(|e| panic!("pdfium-auto: failed to write bundled.rs: {e}"));

    println!("cargo:rerun-if-changed={}", lib_dest.display());
}
