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
//! 2. If absent or truncated, downloads the correct `.tgz` from
//!    [bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries).
//! 3. Extracts via atomic temp-file + `rename`, guarded by an advisory lock.
//! 4. Calls [`Pdfium::bind_to_library`] to load the real library.
//!
//! ## SPEC-095 — cache poison fix
//!
//! Extraction never writes directly to the final path. A short / corrupt file
//! fails the size integrity check and is re-extracted. Concurrent callers
//! serialize on an advisory lock file.
//!
//! ## Environment variable overrides
//!
//! - `PDFIUM_LIB_PATH` — path to an existing pdfium library; skips extract/download.
//! - `PDFIUM_AUTO_CACHE_DIR` — override the base cache directory.
//! - `PDFIUM_BUNDLE_LIB` — (compile time) path to the dylib to embed when
//!   the `bundled` feature is active.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use pdfium_render::prelude::Pdfium;
use thiserror::Error;

// ── Public constants ─────────────────────────────────────────────────────────

/// The pdfium-binaries release tag used for downloads and cache paths.
///
/// Latest stable as of 2026-07-20: [`bblanchon/pdfium-binaries chromium/7961`]
/// (PDFium 152.0.7961.0).
pub const PDFIUM_VERSION: &str = "7961";

/// GitHub release base URL.
const BASE_URL: &str = "https://github.com/bblanchon/pdfium-binaries/releases/download";

/// Minimum plausible shared-library size (guards against truncated poison files
/// when exact expected length is unknown — download mode).
const MIN_LIB_BYTES: u64 = 100_000;

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

    /// Advisory lock acquisition failed.
    #[error("Cache lock error: {0}")]
    Lock(String),

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

// ── Integrity + atomic publish (SPEC-095) ────────────────────────────────────

/// Returns true when `path` exists and has exact `expected_len` bytes.
#[cfg(feature = "bundled")]
fn cache_file_valid_exact(path: &Path, expected_len: u64) -> bool {
    std::fs::metadata(path)
        .map(|m| m.len() == expected_len)
        .unwrap_or(false)
}

/// Returns true when `path` exists and is at least `MIN_LIB_BYTES` (download mode).
fn cache_file_valid_min(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|m| m.len() >= MIN_LIB_BYTES)
        .unwrap_or(false)
}

fn lib_path_override() -> Option<PathBuf> {
    let env_path = std::env::var_os("PDFIUM_LIB_PATH")?;
    let p = PathBuf::from(env_path);
    match std::fs::metadata(&p) {
        Ok(m) if m.len() > 0 => Some(p),
        _ => None,
    }
}

/// Acquire an exclusive advisory lock for extraction into `cache_dir`.
fn acquire_extract_lock(cache_dir: &Path) -> Result<File, PdfiumAutoError> {
    std::fs::create_dir_all(cache_dir).map_err(PdfiumAutoError::CacheDir)?;
    let lock_path = cache_dir.join("pdfium.extract.lock");
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(PdfiumAutoError::CacheDir)?;
    file.lock()
        .map_err(|e| PdfiumAutoError::Lock(format!("{}: {e}", lock_path.display())))?;
    Ok(file)
}

/// Write `bytes` to a unique temp file under the same directory as `dest`,
/// fsync, then atomically `rename` onto `dest`.
fn atomic_publish_bytes(dest: &Path, bytes: &[u8]) -> Result<(), PdfiumAutoError> {
    let parent = dest.parent().ok_or_else(|| {
        PdfiumAutoError::Extract(format!("destination has no parent: {}", dest.display()))
    })?;
    std::fs::create_dir_all(parent).map_err(PdfiumAutoError::CacheDir)?;

    // Unique temp names: pid + monotonic seq (nanos alone can collide under load).
    static TMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = TMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = parent.join(format!(
        "{}.tmp.{}.{}",
        dest.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("libpdfium"),
        std::process::id(),
        seq
    ));

    {
        let mut f = File::create(&tmp)
            .map_err(|e| PdfiumAutoError::Extract(format!("create temp {}: {e}", tmp.display())))?;
        f.write_all(bytes)
            .map_err(|e| PdfiumAutoError::Extract(format!("write temp {}: {e}", tmp.display())))?;
        f.sync_all()
            .map_err(|e| PdfiumAutoError::Extract(format!("fsync temp {}: {e}", tmp.display())))?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&tmp)
            .map_err(PdfiumAutoError::CacheDir)?
            .permissions();
        perms.set_mode(perms.mode() | 0o755);
        std::fs::set_permissions(&tmp, perms).map_err(PdfiumAutoError::CacheDir)?;
    }

    // Windows rename fails if dest exists; Unix replaces atomically.
    #[cfg(windows)]
    {
        let _ = std::fs::remove_file(dest);
    }

    std::fs::rename(&tmp, dest).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        PdfiumAutoError::Extract(format!(
            "rename {} → {}: {e}",
            tmp.display(),
            dest.display()
        ))
    })?;

    Ok(())
}

fn remove_if_invalid(path: &Path, valid: bool) {
    if path.exists() && !valid {
        let _ = std::fs::remove_file(path);
    }
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Returns `true` if the PDFium library is already cached on disk (no network
/// access needed on next call to [`ensure_pdfium_library`]).
///
/// Also returns `true` when `PDFIUM_LIB_PATH` points to an existing file.
pub fn is_pdfium_cached() -> bool {
    if lib_path_override().is_some() {
        return true;
    }
    if let Ok(info) = detect_platform() {
        let p = pdfium_cache_dir().join(info.lib_name);
        return cache_file_valid_min(&p);
    }
    false
}

/// Returns the on-disk path to the PDFium library, or `None` if not cached.
pub fn cached_pdfium_path() -> Option<PathBuf> {
    if let Some(p) = lib_path_override() {
        return Some(p);
    }
    if let Ok(info) = detect_platform() {
        let p = pdfium_cache_dir().join(info.lib_name);
        if cache_file_valid_min(&p) {
            return Some(p);
        }
    }
    None
}

/// Ensures the PDFium dynamic library is present in the local cache.
///
/// - If `PDFIUM_LIB_PATH` is set (and the file exists), that path is used.
/// - Otherwise, checks `pdfium_cache_dir()` for a size-valid library.
/// - If absent or truncated, downloads and atomically extracts.
///
/// # Thread safety
///
/// Safe to call from multiple threads / processes; extraction is guarded by
/// an advisory file lock.
pub fn ensure_pdfium_library(
    on_progress: Option<&dyn Fn(u64, Option<u64>)>,
) -> Result<PathBuf, PdfiumAutoError> {
    if let Some(path) = RESOLVED_PATH.get() {
        return Ok(path.clone());
    }

    let path = resolve_or_download(on_progress)?;
    let _ = RESOLVED_PATH.set(path.clone());
    Ok(path)
}

/// Binds to PDFium, downloading it first if necessary.
pub fn bind_pdfium(
    on_progress: Option<&dyn Fn(u64, Option<u64>)>,
) -> Result<Pdfium, PdfiumAutoError> {
    let lib_path = ensure_pdfium_library(on_progress)?;
    bind_pdfium_from_path(&lib_path)
}

/// Binds to PDFium without any progress output.
pub fn bind_pdfium_silent() -> Result<Pdfium, PdfiumAutoError> {
    bind_pdfium(None)
}

/// Binds to a PDFium library at an explicit `path`.
pub fn bind_pdfium_from_path(path: &Path) -> Result<Pdfium, PdfiumAutoError> {
    Pdfium::bind_to_library(path)
        .map(Pdfium::new)
        .map_err(|e| PdfiumAutoError::Bind {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })
}

// ── Bundled feature ───────────────────────────────────────────────────────────

#[cfg(feature = "bundled")]
mod bundled_lib {
    // `bundled.rs` is generated by build.rs and defines:
    //   pub static PDFIUM_BYTES: &[u8] = include_bytes!("bundled_pdfium_lib");
    include!(concat!(env!("OUT_DIR"), "/bundled.rs"));
}

/// Ensures the embedded PDFium library is extracted to the local cache and
/// returns its on-disk path.
///
/// Honours `PDFIUM_LIB_PATH` (skip extract). Uses atomic publish + size check
/// + advisory lock (SPEC-095).
#[cfg(feature = "bundled")]
pub fn ensure_pdfium_bundled() -> Result<PathBuf, PdfiumAutoError> {
    if let Some(path) = lib_path_override() {
        let _ = RESOLVED_PATH.set(path.clone());
        return Ok(path);
    }

    if let Some(path) = RESOLVED_PATH.get() {
        return Ok(path.clone());
    }

    let info = detect_platform()?;
    let cache_dir = pdfium_cache_dir();
    let lib_path = cache_dir.join(info.lib_name);
    let expected = bundled_lib::PDFIUM_BYTES.len() as u64;

    if cache_file_valid_exact(&lib_path, expected) {
        let _ = RESOLVED_PATH.set(lib_path.clone());
        return Ok(lib_path);
    }

    let _lock = acquire_extract_lock(&cache_dir)?;

    // Re-check under lock (another process may have finished).
    if cache_file_valid_exact(&lib_path, expected) {
        let _ = RESOLVED_PATH.set(lib_path.clone());
        return Ok(lib_path);
    }

    remove_if_invalid(&lib_path, false);
    atomic_publish_bytes(&lib_path, bundled_lib::PDFIUM_BYTES)?;

    if !cache_file_valid_exact(&lib_path, expected) {
        return Err(PdfiumAutoError::Extract(format!(
            "post-extract size mismatch at {}: got {:?}, expected {expected}",
            lib_path.display(),
            std::fs::metadata(&lib_path).map(|m| m.len()).ok()
        )));
    }

    let _ = RESOLVED_PATH.set(lib_path.clone());
    Ok(lib_path)
}

/// Binds to the PDFium library that was embedded at compile time.
#[cfg(feature = "bundled")]
pub fn bind_bundled() -> Result<Pdfium, PdfiumAutoError> {
    let lib_path = ensure_pdfium_bundled()?;
    bind_pdfium_from_path(&lib_path)
}

/// Expected embedded library byte length (bundled feature only).
#[cfg(feature = "bundled")]
pub fn bundled_pdfium_len() -> usize {
    bundled_lib::PDFIUM_BYTES.len()
}

// ── Internal helpers ─────────────────────────────────────────────────────────

fn resolve_or_download(
    on_progress: Option<&dyn Fn(u64, Option<u64>)>,
) -> Result<PathBuf, PdfiumAutoError> {
    if let Some(p) = lib_path_override() {
        return Ok(p);
    }

    if let Ok(env_path) = std::env::var("PDFIUM_LIB_PATH") {
        eprintln!("pdfium-auto: PDFIUM_LIB_PATH '{env_path}' not found or empty; downloading …");
    }

    let info = detect_platform()?;
    let cache_dir = pdfium_cache_dir();
    let lib_path = cache_dir.join(info.lib_name);

    // Honour a size-valid cache before refusing network (CI uses NO_AUTO_DOWNLOAD
    // with a warm cache or PDFIUM_LIB_PATH).
    if cache_file_valid_min(&lib_path) {
        return Ok(lib_path);
    }

    if std::env::var("PDFIUM_NO_AUTO_DOWNLOAD").is_ok() {
        return Err(PdfiumAutoError::Download(
            "auto-download disabled (PDFIUM_NO_AUTO_DOWNLOAD is set); \
             set PDFIUM_LIB_PATH to point at an existing pdfium library"
                .to_string(),
        ));
    }

    let _lock = acquire_extract_lock(&cache_dir)?;

    if cache_file_valid_min(&lib_path) {
        return Ok(lib_path);
    }

    remove_if_invalid(&lib_path, false);

    let url = format!(
        "{BASE_URL}/chromium%2F{PDFIUM_VERSION}/{}",
        info.archive_name
    );

    let archive_bytes = download_bytes(&url, on_progress)?;
    extract_library_atomic(&archive_bytes, info.lib_path_in_archive, &lib_path)?;

    if !cache_file_valid_min(&lib_path) {
        return Err(PdfiumAutoError::Extract(format!(
            "post-extract library too short at {}",
            lib_path.display()
        )));
    }

    Ok(lib_path)
}

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
    let mut chunk = vec![0u8; 64 * 1024];
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

/// Extract library bytes from a gzipped tar into memory, then atomically publish.
fn extract_library_atomic(
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
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|e| PdfiumAutoError::Extract(format!("read archive entry: {e}")))?;
            return atomic_publish_bytes(dest_path, &buf);
        }
    }

    Err(PdfiumAutoError::Extract(format!(
        "Library '{lib_path_in_archive}' not found in archive"
    )))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    /// Serialise env-mutating tests (PDFIUM_AUTO_CACHE_DIR / PDFIUM_LIB_PATH).
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    #[test]
    fn detect_platform_is_supported() {
        detect_platform().expect("current platform should be supported");
    }

    #[test]
    fn cache_dir_is_deterministic() {
        let _g = env_lock();
        std::env::remove_var("PDFIUM_AUTO_CACHE_DIR");
        let d1 = pdfium_cache_dir();
        let d2 = pdfium_cache_dir();
        assert_eq!(d1, d2);
        assert!(
            d1.to_str().unwrap().contains("pdf2md"),
            "expected 'pdf2md' in {d1:?}"
        );
        assert!(
            d1.to_str().unwrap().contains(PDFIUM_VERSION),
            "expected PDFIUM_VERSION {PDFIUM_VERSION} in {d1:?}"
        );
    }

    #[test]
    fn cache_dir_override_via_env() {
        let _g = env_lock();
        std::env::set_var("PDFIUM_AUTO_CACHE_DIR", "/tmp/test_pdf2md_override");
        let d = pdfium_cache_dir();
        std::env::remove_var("PDFIUM_AUTO_CACHE_DIR");
        assert!(d.starts_with("/tmp/test_pdf2md_override"), "left: {d:?}");
        assert!(
            d.to_str().unwrap().contains(PDFIUM_VERSION),
            "expected PDFIUM_VERSION {PDFIUM_VERSION} in {d:?}"
        );
    }

    #[test]
    fn platform_info_fields_nonempty() {
        let info = detect_platform().unwrap();
        assert!(!info.archive_name.is_empty());
        assert!(!info.lib_path_in_archive.is_empty());
        assert!(!info.lib_name.is_empty());
    }

    #[test]
    fn atomic_publish_survives_concurrent_writers() {
        let dir = std::env::temp_dir().join(format!("pdfium-auto-atomic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("libpdfium.so");
        let payload = vec![0xABu8; 256 * 1024];

        let handles: Vec<_> = (0..8)
            .map(|_| {
                let dest = dest.clone();
                let payload = payload.clone();
                std::thread::spawn(move || atomic_publish_bytes(&dest, &payload).unwrap())
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(
            std::fs::metadata(&dest).unwrap().len(),
            payload.len() as u64
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn truncated_file_fails_exact_and_min_checks() {
        let dir = std::env::temp_dir().join(format!("pdfium-auto-trunc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("libpdfium.so");
        std::fs::write(&path, [0u8; 100]).unwrap();
        assert!(!cache_file_valid_min(&path));
        #[cfg(feature = "bundled")]
        assert!(!cache_file_valid_exact(&path, 6_000_000));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(feature = "bundled")]
    #[test]
    fn truncated_cache_is_reextracted() {
        // Full ensure()-based heal lives in tests/poison_heal_ensure.rs (cold process).
        let _g = env_lock();
        let base =
            std::env::temp_dir().join(format!("pdfium-auto-poison-unit-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let lib = base.join("libpdfium.so");
        std::fs::write(&lib, [0u8; 100]).unwrap();
        assert!(!cache_file_valid_exact(
            &lib,
            bundled_lib::PDFIUM_BYTES.len() as u64
        ));
        remove_if_invalid(&lib, false);
        atomic_publish_bytes(&lib, bundled_lib::PDFIUM_BYTES).unwrap();
        assert!(cache_file_valid_exact(
            &lib,
            bundled_lib::PDFIUM_BYTES.len() as u64
        ));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(feature = "bundled")]
    #[test]
    fn lib_path_skips_bundled_extract() {
        // Full ensure()+cache untouched lives in tests/lib_path_skips_extract.rs.
        let _g = env_lock();
        let base =
            std::env::temp_dir().join(format!("pdfium-auto-libpath-unit-{}", std::process::id()));
        let fake_lib = base.join("pinned").join("libpdfium.dylib");
        std::fs::create_dir_all(fake_lib.parent().unwrap()).unwrap();
        std::fs::write(&fake_lib, b"PINNED").unwrap();
        std::env::set_var("PDFIUM_LIB_PATH", &fake_lib);
        assert_eq!(lib_path_override().unwrap(), fake_lib);
        std::env::remove_var("PDFIUM_LIB_PATH");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(feature = "bundled")]
    #[test]
    fn atomic_extract_survives_concurrent_ensure() {
        // True cold-cache concurrent race: tests/cold_cache_concurrent.rs.
        // In-process: only exercise concurrent atomic_publish (RESOLVED_PATH-safe).
        let dir =
            std::env::temp_dir().join(format!("pdfium-auto-conc-unit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("libpdfium.so");
        let payload = bundled_lib::PDFIUM_BYTES.to_vec();
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let dest = dest.clone();
                let payload = payload.clone();
                std::thread::spawn(move || atomic_publish_bytes(&dest, &payload).unwrap())
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(
            std::fs::metadata(&dest).unwrap().len(),
            bundled_lib::PDFIUM_BYTES.len() as u64
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
