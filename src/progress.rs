//! Progress-callback trait for per-page conversion events.
//!
//! Inject an [`Arc<dyn ConversionProgressCallback>`] via
//! [`crate::config::ConversionConfigBuilder::progress_callback`] to receive
//! real-time events as the pipeline processes each page.
//!
//! # Why callbacks instead of channels?
//!
//! The callback approach is the least-invasive integration point: callers can
//! forward events to a Tokio broadcast channel, a WebSocket, a database record,
//! or a terminal progress bar — without the library knowing anything about how
//! the host application communicates. The trait is `Send + Sync` so it works
//! correctly when pages are processed concurrently via `tokio::spawn`.
//!
//! # Example
//!
//! ```rust
//! use edgequake_pdf2md::{ConversionProgressCallback, ConversionConfig};
//! use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
//!
//! struct CountingCallback {
//!     completed: Arc<AtomicUsize>,
//! }
//!
//! impl ConversionProgressCallback for CountingCallback {
//!     fn on_page_complete(&self, page_num: usize, total_pages: usize, markdown_len: usize) {
//!         let done = self.completed.fetch_add(1, Ordering::SeqCst) + 1;
//!         eprintln!("Page {}/{} done ({} bytes)", page_num, total_pages, markdown_len);
//!     }
//! }
//!
//! let counter = Arc::new(CountingCallback {
//!     completed: Arc::new(AtomicUsize::new(0)),
//! });
//!
//! let config = ConversionConfig::builder()
//!     .progress_callback(counter as Arc<dyn ConversionProgressCallback>)
//!     .build()
//!     .unwrap();
//! ```

use std::sync::Arc;

/// Called by the conversion pipeline as it processes each page.
///
/// Implementations must be `Send + Sync` (the pipeline can process pages
/// concurrently via `tokio::spawn`). All methods have default no-op
/// implementations so callers only override what they care about.
///
/// # Thread safety
///
/// When `maintain_format = false`, `on_page_start`, `on_page_complete`, and
/// `on_page_error` may be called concurrently from different threads.
/// Implementations must protect shared mutable state with appropriate
/// synchronisation primitives (e.g. `Mutex`, `AtomicUsize`).
pub trait ConversionProgressCallback: Send + Sync {
    /// Called once before any page is rendered.
    ///
    /// # Arguments
    /// * `total_pages` — number of pages that will be processed
    fn on_conversion_start(&self, total_pages: usize) {
        let _ = total_pages;
    }

    /// Called just before the VLM request is sent for a page.
    ///
    /// # Arguments
    /// * `page_num`    — 1-indexed page number
    /// * `total_pages` — total pages in the document
    fn on_page_start(&self, page_num: usize, total_pages: usize) {
        let _ = (page_num, total_pages);
    }

    /// Called when a page is successfully converted.
    ///
    /// # Arguments
    /// * `page_num`     — 1-indexed page number
    /// * `total_pages`  — total pages
    /// * `markdown_len` — byte length of the produced Markdown
    ///   (useful for progress bars that track output size)
    fn on_page_complete(&self, page_num: usize, total_pages: usize, markdown_len: usize) {
        let _ = (page_num, total_pages, markdown_len);
    }

    /// Called when a page fails after all retries are exhausted.
    ///
    /// # Arguments
    /// * `page_num`    — 1-indexed page number
    /// * `total_pages` — total pages
    /// * `error`       — human-readable error description
    ///
    /// # Note (v0.4.2)
    /// The parameter was changed from `&str` to `String` to eliminate the
    /// higher-ranked trait bound (HRTB) `for<'a> &'a str` that prevented
    /// the future returned by `#[async_trait]` methods from being `Send`.
    /// Callers that previously passed `&e.to_string()` should now pass
    /// `e.to_string()` directly.
    fn on_page_error(&self, page_num: usize, total_pages: usize, error: String) {
        let _ = (page_num, total_pages, error);
    }

    /// Called once after all pages have been attempted.
    ///
    /// # Arguments
    /// * `total_pages`   — total pages in the document
    /// * `success_count` — pages that converted without error
    fn on_conversion_complete(&self, total_pages: usize, success_count: usize) {
        let _ = (total_pages, success_count);
    }
}

/// A no-op implementation for callers that don't need progress events.
///
/// This is the default when no callback is configured.
pub struct NoopProgressCallback;

impl ConversionProgressCallback for NoopProgressCallback {}

/// Convenience alias matching the type stored in [`crate::config::ConversionConfig`].
pub type ProgressCallback = Arc<dyn ConversionProgressCallback>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TrackingCallback {
        starts: Arc<AtomicUsize>,
        completes: Arc<AtomicUsize>,
        errors: Arc<AtomicUsize>,
        started_total: Arc<AtomicUsize>,
        completed_total: Arc<AtomicUsize>,
    }

    impl ConversionProgressCallback for TrackingCallback {
        fn on_conversion_start(&self, total_pages: usize) {
            self.started_total.store(total_pages, Ordering::SeqCst);
        }

        fn on_page_start(&self, _page_num: usize, _total_pages: usize) {
            self.starts.fetch_add(1, Ordering::SeqCst);
        }

        fn on_page_complete(&self, _page_num: usize, _total_pages: usize, _markdown_len: usize) {
            self.completes.fetch_add(1, Ordering::SeqCst);
        }

        fn on_page_error(&self, _page_num: usize, _total_pages: usize, _error: String) {
            self.errors.fetch_add(1, Ordering::SeqCst);
        }

        fn on_conversion_complete(&self, _total_pages: usize, success_count: usize) {
            self.completed_total.store(success_count, Ordering::SeqCst);
        }
    }

    #[test]
    fn noop_callback_does_not_panic() {
        let cb = NoopProgressCallback;
        cb.on_conversion_start(5);
        cb.on_page_start(1, 5);
        cb.on_page_complete(1, 5, 42);
        cb.on_page_error(2, 5, "some error".to_string());
        cb.on_conversion_complete(5, 4);
    }

    #[test]
    fn tracking_callback_receives_events() {
        let tracker = TrackingCallback {
            starts: Arc::new(AtomicUsize::new(0)),
            completes: Arc::new(AtomicUsize::new(0)),
            errors: Arc::new(AtomicUsize::new(0)),
            started_total: Arc::new(AtomicUsize::new(0)),
            completed_total: Arc::new(AtomicUsize::new(0)),
        };

        tracker.on_conversion_start(3);
        assert_eq!(tracker.started_total.load(Ordering::SeqCst), 3);

        tracker.on_page_start(1, 3);
        tracker.on_page_complete(1, 3, 100);
        tracker.on_page_start(2, 3);
        tracker.on_page_complete(2, 3, 200);
        tracker.on_page_start(3, 3);
        tracker.on_page_error(3, 3, "VLM timeout".to_string());

        assert_eq!(tracker.starts.load(Ordering::SeqCst), 3);
        assert_eq!(tracker.completes.load(Ordering::SeqCst), 2);
        assert_eq!(tracker.errors.load(Ordering::SeqCst), 1);

        tracker.on_conversion_complete(3, 2);
        assert_eq!(tracker.completed_total.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn arc_dyn_callback_works() {
        let cb: Arc<dyn ConversionProgressCallback> = Arc::new(NoopProgressCallback);
        cb.on_conversion_start(10);
        cb.on_page_start(1, 10);
        cb.on_page_complete(1, 10, 512);
    }

    /// Regression test for issues #8 and #9.
    ///
    /// Before the fix, `on_page_error` took `error: &str`, which introduced the
    /// HRTB `for<'a> &'a str`. That made the future produced by
    /// `#[async_trait]` methods non-`Send`, causing compilation failures like:
    ///
    ///   error: implementation of `Send` is not general enough
    ///     = note: `Send` would have to be implemented for `&str`
    ///
    /// Moving to `error: String` eliminates the HRTB. This test proves the
    /// callback can be moved into a `tokio::spawn` task (which requires `Send`).
    #[tokio::test]
    async fn on_page_error_is_send_when_used_in_spawn() {
        use std::sync::Mutex;

        struct StringCollector {
            errors: Arc<Mutex<Vec<String>>>,
        }

        impl ConversionProgressCallback for StringCollector {
            fn on_page_error(&self, _page_num: usize, _total_pages: usize, error: String) {
                self.errors.lock().unwrap().push(error);
            }
        }

        let collector = Arc::new(StringCollector {
            errors: Arc::new(Mutex::new(Vec::new())),
        });

        let cb: Arc<dyn ConversionProgressCallback> =
            Arc::clone(&collector) as Arc<dyn ConversionProgressCallback>;

        // Moving cb into tokio::spawn proves Arc<dyn ConversionProgressCallback>
        // is Send — which in turn requires all &self method args to be 'static.
        tokio::spawn(async move {
            cb.on_page_error(1, 5, "error from spawn".to_string());
        })
        .await
        .unwrap();

        let errors = collector.errors.lock().unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0], "error from spawn");
    }

    /// Extra: verify the error String is forwarded by value (no copy/clone overhead).
    #[test]
    fn on_page_error_receives_owned_string() {
        use std::sync::Mutex;

        struct ErrorCapture {
            captured: Arc<Mutex<Option<String>>>,
        }

        impl ConversionProgressCallback for ErrorCapture {
            fn on_page_error(&self, _p: usize, _t: usize, error: String) {
                *self.captured.lock().unwrap() = Some(error);
            }
        }

        let capture = ErrorCapture {
            captured: Arc::new(Mutex::new(None)),
        };

        let long_error = "x".repeat(200);
        capture.on_page_error(3, 10, long_error.clone());

        let got = capture.captured.lock().unwrap().clone().unwrap();
        assert_eq!(got, long_error, "Full error string should be forwarded");
    }
}
