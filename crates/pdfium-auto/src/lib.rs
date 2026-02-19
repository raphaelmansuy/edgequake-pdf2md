//! # pdfium-auto
//!
//! Auto-download and cache [PDFium](https://pdfium.googlesource.com/pdfium/)
//! binaries at runtime, so that users of `pdfium-render` no longer need to
//! manually download libpdfium and set `DYLD_LIBRARY_PATH` / `LD_LIBRARY_PATH`.
//!
//! ## How it works
//!
//! On first call to [`bind_pdfium`] or [`ensure_pdfium_library`]:
//!
//! 1. Checks `~/.cache/pdf2md/pdfium-{VERSION}/` for the platform library.
//! 2. If absent, downloads the correct `.tgz` from
//!    [bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries).
//! 3. Extracts `lib/libpdfium.dylib` (or `.so` / `.dll`) to the cache dir.
//! 4. Calls [`Pdfium::bind_to_library`] to load the real library.
//!
//! Subsequent calls skip the network entirely — the library is already cached.
//!
//! ## Usage
//!
//! ```rust,no_run
//! use pdfium_auto::{bind_pdfium_silent, bind_pdfium_from_path, ensure_pdfium_library};
//!
//! // Option A: convenient one-shot bind (silent, no progress)
//! let pdfium = bind_pdfium_silent().expect("PDFium unavailable");
//!
//! // Option B: download with progress, then bind
//! let path = ensure_pdfium_library(Some(&|downloaded, total| {
//!     if let Some(t) = total {
//!         eprint!("\rDownloading PDFium: {}/{} bytes", downloaded, t);
//!     }
//! })).expect("download failed");
//! let pdfium = bind_pdfium_from_path(&path).expect("bind failed");
//! ```
//!
//! ## Platform support
//!
//! | OS      | Arch    | Library               |
//! |---------|---------|-----------------------|
//! | macOS   | arm64   | `libpdfium.dylib`     |
//! | macOS   | x86_64  | `libpdfium.dylib`     |
//! | Linux   | x86_64  | `libpdfium.so`        |
//! | Linux   | aarch64 | `libpdfium.so`        |
//! | Windows | x86_64  | `pdfium.dll`          |
//! | Windows | aarch64 | `pdfium.dll`          |
//! | Windows | x86     | `pdfium.dll`          |
//!
//! ## Environment variable overrides
//!
//! - `PDFIUM_LIB_PATH` — path to an existing pdfium library; skips download.
//! - `PDFIUM_AUTO_CACHE_DIR` — override the default cache directory.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use pdfium_render::prelude::Pdfium;
use thiserror::Error;

// ── Public constants ─────────────────────────────────────────────────────────

/// The pdfium-binaries release tag used for downloads.
///
/// Maps to [`bblanchon/pdfium-binaries chromium/7690`](https://github.com/bblanchon/pdfium-binaries/releases/tag/chromium%2F7690).
pub const PDFIUM_VERSION: &str = "7690";

/// GitHub release base URL.
const BASE_URL: &str = "https://github.com/bblanchon/pdfium-binaries/releases/download";

// ── Error type ───────────────────────────────────────────────────────────────

/// Errors returned by pdfium-auto operations.
#[derive(Error, Debug)]
pub enum PdfiumAutoError {
    /// The current OS/architecture combination is not supported.
    #[error("Unsupported platform: {os}/{arch}")]
    UnsupportedPlatform { os: String, arch: String },

    /// Could not create or navigate the local cache directory.
    #[error("Cache directory error: {0}")]
    CacheDir(#[source] std::io::Error),

    /// Network download failed.
    #[error("Download failed: {0}")]
    Download(String),

    /// gzip/tar extraction failed.
    #[error("Archive extraction failed: {0}")]
    Extract(String),

    /// `libloading` / `pdfium-render` could not load the library.
    #[error("Failed to bind PDFium from '{path}': {reason}")]
    Bind { path: PathBuf, reason: String },
}

// ── Internal: platform metadata ──────────────────────────────────────────────

struct PlatformInfo {
    /// Asset filename in the GitHub release, e.g. `pdfium-mac-arm64.tgz`.
    archive_name: &'static str,
    /// Relative path inside the archive, e.g. `lib/libpdfium.dylib`.
    lib_path_in_archive: &'static str,
    /// Filename to write on disk, e.g. `libpdfium.dylib`.
    lib_name: &'static str,
}

fn detect_platform() -> Result<PlatformInfo, PdfiumAutoError> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    match (os, arch) {
        ("macos", "aarch64") => Ok(PlatformInfo {
            archive_name: "pdfium-mac-arm64.tgz",
            lib_path_in_archive: "lib/libpdfium.dylib",
            lib_name: "libpdfium.dylib",
        }),
        ("macos", "x86_64") => Ok(PlatformInfo {
            archive_name: "pdfium-mac-x64.tgz",
            lib_path_in_archive: "lib/libpdfium.dylib",
            lib_name: "libpdfium.dylib",
        }),
        ("linux", "x86_64") => Ok(PlatformInfo {
            archive_name: "pdfium-linux-x64.tgz",
            lib_path_in_archive: "lib/libpdfium.so",
            lib_name: "libpdfium.so",
        }),
        ("linux", "aarch64") => Ok(PlatformInfo {
            archive_name: "pdfium-linux-arm64.tgz",
            lib_path_in_archive: "lib/libpdfium.so",
            lib_name: "libpdfium.so",
        }),
        ("windows", "x86_64") => Ok(PlatformInfo {
            archive_name: "pdfium-win-x64.tgz",
            lib_path_in_archive: "bin/pdfium.dll",
            lib_name: "pdfium.dll",
        }),
        ("windows", "aarch64") => Ok(PlatformInfo {
            archive_name: "pdfium-win-arm64.tgz",
            lib_path_in_archive: "bin/pdfium.dll",
            lib_name: "pdfium.dll",
        }),
        ("windows", "x86") => Ok(PlatformInfo {
            archive_name: "pdfium-win-x86.tgz",
            lib_path_in_archive: "bin/pdfium.dll",
            lib_name: "pdfium.dll",
        }),
        (os, arch) => Err(PdfiumAutoError::UnsupportedPlatform {
            os: os.to_string(),
            arch: arch.to_string(),
        }),
    }
}

// ── Cache directory resolution ───────────────────────────────────────────────

/// Returns the per-version cache directory for the PDFium library.
///
/// Default locations:
/// - **macOS**: `~/Library/Caches/pdf2md/pdfium-{VERSION}/`
/// - **Linux**: `~/.cache/pdf2md/pdfium-{VERSION}/`
/// - **Windows**: `%LOCALAPPDATA%\pdf2md\pdfium-{VERSION}\`
///
/// Override by setting `PDFIUM_AUTO_CACHE_DIR`.
pub fn pdfium_cache_dir() -> PathBuf {
    if let Ok(override_dir) = std::env::var("PDFIUM_AUTO_CACHE_DIR") {
        return PathBuf::from(override_dir).join(format!("pdfium-{PDFIUM_VERSION}"));
    }

    let base = dirs::cache_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".cache")))
        .unwrap_or_else(std::env::temp_dir);

    base.join("pdf2md").join(format!("pdfium-{PDFIUM_VERSION}"))
}

// ── Thread-safe singleton path cache ─────────────────────────────────────────

static RESOLVED_PATH: OnceLock<PathBuf> = OnceLock::new();

// ── Public API ───────────────────────────────────────────────────────────────

/// Returns `true` if the PDFium library is already cached on disk (no network
/// access needed on next call to [`ensure_pdfium_library`]).
///
/// Also returns `true` when `PDFIUM_LIB_PATH` points to an existing file.
pub fn is_pdfium_cached() -> bool {
    if let Ok(p) = std::env::var("PDFIUM_LIB_PATH") {
        return PathBuf::from(p).exists();
    }
    if let Ok(info) = detect_platform() {
        return pdfium_cache_dir().join(info.lib_name).exists();
    }
    false
}

/// Returns the on-disk path to the PDFium library, or `None` if not cached.
pub fn cached_pdfium_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("PDFIUM_LIB_PATH") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    if let Ok(info) = detect_platform() {
        let p = pdfium_cache_dir().join(info.lib_name);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Ensures the PDFium dynamic library is present in the local cache.
///
/// - If `PDFIUM_LIB_PATH` is set (and the file exists), that path is used.
/// - Otherwise, checks `pdfium_cache_dir()` for an existing library.
/// - If absent, downloads the appropriate platform binary from GitHub
///   and extracts it to the cache directory.
///
/// `on_progress` receives `(bytes_downloaded, total_size_option)` during
/// the download.  Pass `None` to suppress progress callbacks.
///
/// # Thread safety
///
/// Safe to call from multiple threads simultaneously; the download happens
/// only once per process lifetime.
pub fn ensure_pdfium_library(
    on_progress: Option<&dyn Fn(u64, Option<u64>)>,
) -> Result<PathBuf, PdfiumAutoError> {
    // Fast path: already resolved in this process.
    if let Some(path) = RESOLVED_PATH.get() {
        return Ok(path.clone());
    }

    let path = resolve_or_download(on_progress)?;

    // Best-effort cache in the OnceLock (ignore race; both will succeed).
    let _ = RESOLVED_PATH.set(path.clone());

    Ok(path)
}

/// Binds to PDFium, downloading it first if necessary.
///
/// `on_progress` receives `(bytes_downloaded, total_bytes_option)` during
/// the initial download.
pub fn bind_pdfium(
    on_progress: Option<&dyn Fn(u64, Option<u64>)>,
) -> Result<Pdfium, PdfiumAutoError> {
    let lib_path = ensure_pdfium_library(on_progress)?;
    bind_pdfium_from_path(&lib_path)
}

/// Binds to PDFium without any progress output.
///
/// Downloads and caches on first call if required.
pub fn bind_pdfium_silent() -> Result<Pdfium, PdfiumAutoError> {
    bind_pdfium(None)
}

/// Binds to a PDFium library at an explicit `path`.
///
/// Does not interact with the download / cache layer.
pub fn bind_pdfium_from_path(path: &Path) -> Result<Pdfium, PdfiumAutoError> {
    Pdfium::bind_to_library(path)
        .map(Pdfium::new)
        .map_err(|e| PdfiumAutoError::Bind {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })
}

// ── Internal helpers ─────────────────────────────────────────────────────────

fn resolve_or_download(
    on_progress: Option<&dyn Fn(u64, Option<u64>)>,
) -> Result<PathBuf, PdfiumAutoError> {
    // 1. Environment variable override.
    if let Ok(env_path) = std::env::var("PDFIUM_LIB_PATH") {
        let p = PathBuf::from(env_path);
        if p.exists() {
            return Ok(p);
        }
        // Fall through: env var set but file missing → still auto-download.
        eprintln!(
            "pdfium-auto: PDFIUM_LIB_PATH '{}' not found; downloading …",
            p.display()
        );
    }

    let info = detect_platform()?;
    let cache_dir = pdfium_cache_dir();
    let lib_path = cache_dir.join(info.lib_name);

    // 2. Already cached on disk.
    if lib_path.exists() {
        return Ok(lib_path);
    }

    // 3. Download and extract.
    let url = format!(
        "{}/chromium%2F{}/{}",
        BASE_URL, PDFIUM_VERSION, info.archive_name
    );

    std::fs::create_dir_all(&cache_dir).map_err(PdfiumAutoError::CacheDir)?;

    let archive_bytes = download_bytes(&url, on_progress)?;
    extract_library(&archive_bytes, info.lib_path_in_archive, &lib_path)?;

    Ok(lib_path)
}

/// Streams a URL into a `Vec<u8>`, calling `on_progress` every 64 KiB.
fn download_bytes(
    url: &str,
    on_progress: Option<&dyn Fn(u64, Option<u64>)>,
) -> Result<Vec<u8>, PdfiumAutoError> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("pdfium-auto/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| PdfiumAutoError::Download(e.to_string()))?;

    let response = client
        .get(url)
        .send()
        .map_err(|e| PdfiumAutoError::Download(format!("GET {url}: {e}")))?;

    if !response.status().is_success() {
        return Err(PdfiumAutoError::Download(format!(
            "HTTP {} for {url}",
            response.status()
        )));
    }

    let total = response.content_length();
    let capacity = total.unwrap_or(35 * 1024 * 1024) as usize;
    let mut buf = Vec::with_capacity(capacity);

    let mut stream = response;
    let mut chunk = vec![0u8; 64 * 1024]; // 64 KiB
    let mut downloaded: u64 = 0;

    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                downloaded += n as u64;
                if let Some(cb) = on_progress {
                    cb(downloaded, total);
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                return Err(PdfiumAutoError::Download(format!("Read error: {e}")));
            }
        }
    }

    Ok(buf)
}

/// Extracts a single file from a gzipped tar archive into `dest_path`.
fn extract_library(
    archive_bytes: &[u8],
    lib_path_in_archive: &str,
    dest_path: &Path,
) -> Result<(), PdfiumAutoError> {
    use flate2::read::GzDecoder;
    use tar::Archive;

    let gz = GzDecoder::new(archive_bytes);
    let mut archive = Archive::new(gz);

    for entry in archive
        .entries()
        .map_err(|e| PdfiumAutoError::Extract(e.to_string()))?
    {
        let mut entry = entry.map_err(|e| PdfiumAutoError::Extract(e.to_string()))?;
        let entry_path = entry
            .path()
            .map_err(|e| PdfiumAutoError::Extract(e.to_string()))?;

        let entry_str = entry_path.to_string_lossy();
        if entry_str == lib_path_in_archive {
            entry
                .unpack(dest_path)
                .map_err(|e| PdfiumAutoError::Extract(format!("Unpack failed: {e}")))?;
            return Ok(());
        }
    }

    Err(PdfiumAutoError::Extract(format!(
        "Library '{}' not found in archive",
        lib_path_in_archive
    )))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_platform_is_supported() {
        // Verify the current platform is recognised.
        detect_platform().expect("current platform should be supported");
    }

    #[test]
    fn cache_dir_is_deterministic() {
        let d1 = pdfium_cache_dir();
        let d2 = pdfium_cache_dir();
        assert_eq!(d1, d2);
        assert!(d1.to_str().unwrap().contains("pdf2md"));
        assert!(d1.to_str().unwrap().contains(PDFIUM_VERSION));
    }

    #[test]
    fn cache_dir_override_via_env() {
        std::env::set_var("PDFIUM_AUTO_CACHE_DIR", "/tmp/test_pdf2md_override");
        let d = pdfium_cache_dir();
        std::env::remove_var("PDFIUM_AUTO_CACHE_DIR");
        assert!(d.starts_with("/tmp/test_pdf2md_override"));
        assert!(d.to_str().unwrap().contains(PDFIUM_VERSION));
    }

    #[test]
    fn platform_info_fields_nonempty() {
        let info = detect_platform().unwrap();
        assert!(!info.archive_name.is_empty());
        assert!(!info.lib_path_in_archive.is_empty());
        assert!(!info.lib_name.is_empty());
    }
}
