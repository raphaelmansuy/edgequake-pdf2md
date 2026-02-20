# Changelog

All notable changes to `edgequake-pdf2md` will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

---

## [0.4.4] — 2026-02-20

### Fixed

- **Issue #13 — `gpt-4.1-nano` (and o1/o3/o4-mini) rejected all PDF vision
  requests** with `400 Bad Request: 'max_tokens' is not supported with this
  model. Use 'max_completion_tokens' instead.`

  Root cause: `edgequake-llm ≤ 0.2.4` passed `max_tokens` in the OpenAI
  request body for every model. The gpt-4.1 family and o-series only accept
  `max_completion_tokens`.

  Fix: bump dependency `edgequake-llm` `0.2.4` → `0.2.5`. The 0.2.5 release
  upgrades `async-openai` from 0.24 → 0.33, which exposes
  `max_completion_tokens` natively and uses it unconditionally (valid for all
  current chat models).

### Changed

- **`edgequake-llm` dependency bumped** `0.2.4` → `0.2.5`.
  - 0.2.5: async-openai 0.24 → 0.33 upgrade; `max_tokens` → `max_completion_tokens`
    routing; cache-hit + reasoning-token extraction; 23 new tests.

### Tests

- `test_issue13_max_tokens_config_builds_for_gpt41_nano` — always-run: verifies
  `ConversionConfig::builder().max_tokens(2048)` round-trips the field without
  panic (compile-time + config-layer regression guard).
- `test_gpt41_nano_max_completion_tokens_regression` — gated e2e test (requires
  `E2E_ENABLED=1` and `OPENAI_API_KEY`): converts a PDF page with `gpt-4.1-nano`
  and `max_tokens=2048`; fails if the `400 Bad Request` from issue #13 recurs.

---

## [0.4.3] — 2026-02-20

### Added

#### Mistral AI provider support (`pixtral-12b-2409`)

- **Mistral as a first-class provider** via `edgequake-llm 0.2.4` (Mistral
  added in 0.2.3; 0.2.4 is a docs/Python bindings bump with no Rust API
  changes).

  Use Mistral for PDF conversion by setting `MISTRAL_API_KEY`:
  ```bash
  export MISTRAL_API_KEY=your-key
  pdf2md document.pdf -o output.md            # auto-selects pixtral-12b-2409
  pdf2md --provider mistral document.pdf      # explicit, same default model
  pdf2md --provider mistral --model pixtral-12b-2409 document.pdf
  ```

- **Vision-aware model default for Mistral**: `pixtral-12b-2409` is set as
  the automatic default when `--provider mistral` is used (or
  `MISTRAL_API_KEY` is the only key set) without an explicit `--model`. The
  Mistral SDK default (`mistral-small-latest`) is **not** vision-capable and
  would fail on every page; this prevents a silent misuse footgun.

- **Auto-detection chain updated** in `resolve_provider`: after the OpenAI
  preference block, `MISTRAL_API_KEY` is now checked explicitly so the
  correct pixtral default is applied even when the factory's generic
  `from_env()` would otherwise select an incompatible model.

- **Mistral row** added to `--help` provider table and `ENVIRONMENT VARIABLES`
  section in the CLI (`pdf2md --help`).

#### Mistral models reference

| Model | Context | Vision | Notes |
|-------|---------|--------|-------|
| `pixtral-12b-2409` | 128K | ✓ | Recommended for PDF conversion |
| `mistral-small-latest` | 32K | ✗ | Text-only |
| `mistral-large-latest` | 128K | ✗ | Text-only |

#### New tests

- `test_default_vision_model_mistral_variants` — unit test: all Mistral name
  aliases map to `pixtral-12b-2409`
- `test_default_vision_model_other_providers` — unit test: non-Mistral
  providers fall back to `gpt-4.1-nano`
- `test_mistral_config_builder_accepts_provider_name` — structural: config
  builder accepts `provider_name = "mistral"` without errors
- `test_mistral_pixtral_model_available` — structural: `pixtral-12b-2409`
  appears in `MistralProvider::available_models()` catalogue
- `test_pixtral_supports_vision` — structural: pixtral has the expected 128K
  context window
- `test_mistral_pdf_conversion` — gated e2e test (requires `E2E_ENABLED=1` +
  `MISTRAL_API_KEY`): converts a single PDF page with pixtral

### Changed

#### Dependency bump: `edgequake-llm` 0.2.2 → 0.2.4

- `0.2.3`: Added `MistralProvider` with pixtral-12b-2409 vision support
- `0.2.4`: Documentation + `edgequake-litellm` Python bindings (no Rust API
  changes)

### Migration

No breaking changes. Existing code and deployments continue to work
unchanged. Mistral support is purely additive.

---

## [0.4.2] — 2026-02-20

### Fixed

#### Issues #8 and #9 — `on_page_error` HRTB / `Send` fix

- **Breaking change (minor)**: `ConversionProgressCallback::on_page_error`
  now takes `error: String` instead of `error: &str`.

  **Why**: The `&str` parameter introduced a higher-ranked trait bound
  (`for<'a> &'a str`) that prevented the `Future` produced by
  `#[async_trait]` method implementations from being `Send`. Callers using
  `edgequake-pdf2md` from an `#[async_trait]` `impl Something` (e.g. a
  Tokio server task processor) would see:

  ```
  error: implementation of `Send` is not general enough
    = note: `Send` would have to be implemented for `&str`
  ```

  Changing the parameter to `String` eliminates the HRTB and makes the
  future `Send` unconditionally.

  **Migration**: update any `impl ConversionProgressCallback` you have:
  ```rust
  // Before
  fn on_page_error(&self, page: usize, total: usize, error: &str) { … }
  // After
  fn on_page_error(&self, page: usize, total: usize, error: String) { … }
  ```
  If you were passing the error to a function that takes `&str`, use
  `error.as_str()` or `&error`.

- Internal call sites in `convert.rs` updated to pass `e.to_string()` (no
  `&` prefix) — no more temporary-borrow HRTB at the pipeline level.

### Added

- Unit test `progress::tests::on_page_error_is_send_when_used_in_spawn`:
  moves an `Arc<dyn ConversionProgressCallback>` into `tokio::spawn` to
  prove the future is `Send` at compile time.
- Unit test `progress::tests::on_page_error_receives_owned_string`:
  verifies the error `String` is forwarded by value without truncation.
- Integration tests `test_callback_send_in_tokio_spawn` and
  `test_noop_callback_is_send_sync` added to `tests/e2e.rs` (always run,
  no API key required).

---

## [0.4.1] — 2026-02-19

### Fixed

- **CI/CD publish workflow**: `CARGO_REGISTRY_TOKEN` is now properly used in the
  publish workflow — automated `cargo publish` on tag push is fully operational.
- Minor version bump to enable automated crates.io publish via GitHub Actions
  (v0.4.0 was published manually due to missing token; no functional changes).

---

## [0.4.0] — 2026-02-19

### Changed — **Breaking feature default**

- **`bundled` is now the default feature** (`default = ["cli", "bundled"]`).
  `cargo install edgequake-pdf2md` produces a fully self-contained binary with
  pdfium (~5 MB) embedded inside — no runtime download, no env vars required.

### Added

- **Auto-download in `build.rs`** when `bundled` feature is active and
  `PDFIUM_BUNDLE_LIB` is not set: `build.rs` downloads the correct pdfium
  release archive for the build target using `curl` and caches it in
  `~/.cargo/pdfium-bundle/{VERSION}/{TARGET_OS}-{TARGET_ARCH}/`.
  - Override cache root: `PDFIUM_BUILD_CACHE_DIR=/path`
  - Opt out of auto-download: set `PDFIUM_BUNDLE_LIB=/path/to/libpdfium`
  - Opt out of bundling entirely:
    `cargo install edgequake-pdf2md --no-default-features --features cli`

- **CI bundled build matrix** — verifies self-contained binary builds on:
  - macOS arm64 (`macos-latest`)
  - macOS x86_64 (`macos-13`)
  - Linux x86_64 (`ubuntu-latest`)
  - Linux aarch64 (`ubuntu-24.04-arm`)
  - Windows x86_64 (`windows-latest`)

- **`pdfium-auto` v0.3.0**: `build.rs` auto-download; `[build-dependencies]`
  (`flate2`, `tar`) added for archive extraction.

### Fixed

- `cargo install edgequake-pdf2md` now works out of the box — pdfium is
  downloaded and embedded at compile time, not at first run.

---

## [0.3.1] — 2026-02-19

### Added

- **`bundled` feature** — embed the pdfium shared library inside the binary at
  compile time, producing a single self-contained executable with no runtime
  dependencies (no internet download, no pre-installed library).
  Build with `PDFIUM_BUNDLE_LIB=/path/to/libpdfium.dylib cargo build --release --features bundled`.
  Supported on macOS arm64/x86_64, Linux x86_64/aarch64, Windows x86_64/aarch64/x86.

### Fixed

- **OpenAI-first provider selection** — when multiple API keys are available
  (`OPENAI_API_KEY`, `GEMINI_API_KEY`, etc.), `gpt-4.1-nano` is now selected by
  default instead of Gemini, matching the documented behaviour.

### Changed

- CI now uses `--features cli` instead of `--all-features` (the `bundled`
  feature requires `PDFIUM_BUNDLE_LIB` at compile time and cannot run
  unattended in CI).

---

## [0.3.0] — 2025-06-06

### Added

#### Zero-friction PDFium setup — `crates/pdfium-auto` (new crate)

A new `pdfium-auto` sub-crate eliminates all manual pdfium installation steps.
Previously, users had to run `./scripts/setup-pdfium.sh`, manually set
`DYLD_LIBRARY_PATH` / `LD_LIBRARY_PATH`, or configure the library path before
running `pdf2md`. Starting from v0.3.0, the correct binary is fetched
automatically.

**How it works:**

1. On first call, detects the current OS / architecture.
2. Downloads the matching `.tgz` from
   [bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries)
   (chromium/7690, ~30 MB) with a live progress bar in the CLI.
3. Extracts `lib/libpdfium.dylib` (or `.so` / `.dll`) to
   `~/.cache/pdf2md/pdfium-7690/` and caches it permanently.
4. Loads the library via `Pdfium::bind_to_library` — no `DYLD_LIBRARY_PATH` needed.
5. All subsequent runs skip the network entirely — the cached path is reused in
   a process-wide `OnceLock<PathBuf>`.

**Platform support**: macOS arm64 / x86_64, Linux x86_64 / aarch64,
Windows x86_64 / aarch64 / x86.

**Public API additions in `pdfium-auto`:**
- `bind_pdfium_silent()` — one-shot bind, no progress callback
- `bind_pdfium(on_progress)` — bind with optional byte-progress callback
- `bind_pdfium_from_path(path)` — explicit path binding
- `ensure_pdfium_library(on_progress)` — download-and-cache only (no bind)
- `is_pdfium_cached()` — synchronous check, no network
- `cached_pdfium_path()` — returns `Option<PathBuf>` if already cached
- `pdfium_cache_dir()` — returns the platform cache directory

**Environment variable overrides:**
- `PDFIUM_LIB_PATH` — skip download, use an existing library at this path
- `PDFIUM_AUTO_CACHE_DIR` — override the default cache directory

#### CLI download progress bar

When `pdf2md` is run for the first time (and pdfium is not yet cached), a
green progress bar like the following appears before conversion begins:

```
Downloading PDFium  [████████████████░░░░░░░░░░░░] 18.2 MiB / 30.5 MiB  ETA 00:00:08
```

Once cached, this step takes zero milliseconds and is skipped silently.

### Changed

- **Cargo workspace** — the project is now a multi-crate workspace
  (`edgequake-pdf2md` + `pdfium-auto`).
- `render.rs` — both `render_pages_blocking` and `extract_metadata_blocking` now
  call `pdfium_auto::bind_pdfium_silent()` instead of `Pdfium::default()`.
- Simplified the `PdfiumBindingFailed` error message to guide users to
  `PDFIUM_LIB_PATH` rather than the now-obsolete `DYLD_LIBRARY_PATH` approach.

### Removed

- `DYLD_LIBRARY_PATH` / `LD_LIBRARY_PATH` from the CLI `SETUP:` help block.
  These env vars are no longer required for normal operation.

---

## [0.2.1] — 2026-02-19

### Added

#### CLI — Rich terminal progress bar (`src/bin/pdf2md.rs`)

The `pdf2md` CLI now wires the `ConversionProgressCallback` API into a live
[indicatif](https://github.com/console-rs/indicatif) progress bar and per-page
log lines, giving users real-time feedback on long conversions.

**UX flow:**

1. **Spinner phase** — while the PDF is being opened and inspected: `⠹ Preparing  Opening PDF…`
2. **Bar phase** — once the page count is known, the spinner transitions to a full progress bar:
   ```
   ⠙ Converting  [████████████████░░░░░░░░░░░░] 12/20 pages  ⏱ 00:00:45  ETA 00:00:15
   ```
3. **Per-page log lines** scroll above the bar as each page completes:
   ```
     ✓ Page   3/20     2 894 chars  7.3s
     ✗ Page   7/20   API timeout                                        8.0s
   ```
4. **Completion summary** once all pages are done:
   ```
   ✔ 19 pages converted successfully
   ✔  19/20 pages  52 341ms  →  output.md
      72 191 tokens in  /  15 882 tokens out
   ```

**Key design decisions:**
- Tracing `INFO` logs are suppressed when the progress bar is active (they'd overwrite the bar). Use `--verbose` to re-enable them.
- `--no-progress` disables the bar and falls back to plain `INFO`-level tracing output.
- `--quiet` suppresses all output (including the bar) — useful in CI and shell pipelines.
- `--json` implies no progress bar (JSON is written to stdout; progress would corrupt it).
- All ANSI colour codes use raw escape sequences — no additional dependencies.

**Fixed:**
- `on_conversion_start` now fires with the count of selected pages (`page_indices.len()`), not the full PDF page count. Previously a `--pages 1-3` run on a 100-page PDF would show `3/100 pages` instead of `3/3`.
- Summary lines (`✔  X/Y pages`) use `processed + failed + skipped` as the denominator, so `--pages 1-3` always shows `3/3`, not the raw PDF total.

---

## [0.2.0] — 2026-05-28

### Added

#### Issue #1 — Per-page progress callbacks
- New `ConversionProgressCallback` trait with default-no-op methods:
  - `on_conversion_start(total_pages)`
  - `on_page_start(page_num, total_pages)`
  - `on_page_complete(page_num, total_pages, markdown_len)`
  - `on_page_error(page_num, total_pages, error)`
  - `on_conversion_complete(total_pages, success_count)`
- `NoopProgressCallback` — zero-cost default implementation (no logging)
- `ProgressCallback` type alias: `Arc<dyn ConversionProgressCallback>`
- `ConversionConfig::progress_callback(cb)` builder method
- Callback hooks wired into `convert()`, `process_concurrent()`, and `process_sequential()` in `convert.rs`
- Re-exported from the crate root: `ConversionProgressCallback`, `NoopProgressCallback`, `ProgressCallback`

#### Issue #2 — In-memory PDF input (`convert_from_bytes`)
- `convert_from_bytes(bytes: &[u8], config: &ConversionConfig) -> Result<ConversionOutput, Pdf2MdError>` — written to a managed `NamedTempFile` internally; caller never manages temp files
- `convert_stream_from_bytes(bytes: &[u8], config: &ConversionConfig) -> Result<PageStream, Pdf2MdError>` — streaming equivalent
- Both functions re-exported from the crate root

#### Issue #3 — Documented provider injection via `Arc<dyn LLMProvider>`
- `ConversionConfig::provider(p: Arc<dyn LLMProvider>)` builder method is now fully documented with resolution-order guarantee:
  1. `config.provider` (highest priority)
  2. `config.provider_name` + `config.model`
  3. `EDGEQUAKE_LLM_PROVIDER` + `EDGEQUAKE_MODEL` env vars
  4. `ProviderFactory::from_env()` (auto-detect)

#### Issue #4 — Richer `Pdf2MdError` variants
- `Pdf2MdError::PartialFailure { success, failed, total }` — returned by `ConversionOutput::into_result()` when any pages fail
- `Pdf2MdError::RateLimitExceeded { provider, retry_after_secs }` — HTTP 429 from VLM API
- `Pdf2MdError::ApiTimeout { page, elapsed_ms }` — per-page API call timed out
- `Pdf2MdError::AuthError { provider, detail }` — HTTP 401/403 from VLM API

#### Output ergonomics
- `ConversionOutput::failed_pages() -> usize` — convenience wrapper around `stats.failed_pages`
- `ConversionOutput::into_result() -> Result<Self, Pdf2MdError>` — promote partial failure to an error

### Changed
- Crate version bumped to `0.2.0`
- `lib.rs` re-exports updated to include all new public API surface

---

- `convert(input, config)` — async, eager conversion: renders all pages in
  parallel, returns a [`ConversionOutput`] with assembled Markdown and per-page
  statistics.
- `convert_stream(input, config)` — streaming variant that yields
  `ConversionEvent` items as each page finishes. Ideal for progress bars and
  large documents where first-page latency matters.
- `convert_to_file(input, output_path, config)` — convenience wrapper that
  writes the assembled Markdown to disk.
- `inspect(input, config)` — metadata-only pass (page count, PDF version,
  author, title, …) without spending any API tokens.

#### Config
- `ConversionConfig` builder with 18 tuneable fields:
  - `dpi` (72–400, default 150) — rasterisation resolution
  - `max_rendered_pixels` (default 2 000 px) — OOM-safety cap for large pages
  - `concurrency` (default 10) — parallel VLM call limit
  - `model` / `provider_name` / `provider` — three-level provider selection
  - `temperature` (default 0.1) — low for faithful transcription
  - `max_tokens` (default 4 096) — per-page output budget
  - `max_retries` / `retry_backoff_ms` — exponential backoff for transient errors
  - `maintain_format` — sequential mode: prior-page context for continuity
  - `fidelity` (`Tier1` / `Tier2` (default) / `Tier3`) — prompt complexity vs. cost
  - `pages` (`All` / `Single` / `Range` / `Set`) — partial-document conversion
  - `page_separator` (`None` / `HorizontalRule` / `Comment` / `Custom`)
  - `include_metadata` — optional YAML front-matter
  - `password` — encrypted PDF support
  - `system_prompt` — fully overridable system instruction
  - `download_timeout_secs` / `api_timeout_secs` — per-operation timeouts

#### Pipeline
- **Input stage**: accepts local file paths and HTTP/HTTPS URLs (streamed to a
  temp file; pdfium requires a filesystem path).
- **Render stage**: pdfium-render rasterises each page to PNG at the configured
  DPI; `spawn_blocking` keeps the async executor unblocked.
- **Encode stage**: PNG is Base64-encoded and wrapped as an OpenAI
  `image_url` part with `detail: "high"` (10-tile tiling for dense text).
- **LLM stage**: multimodal message sent to any Vision LLM supported by
  `edgequake-llm`; per-page retry with exponential backoff.
- **Post-process stage**: rule-based artefact removal (page numbers,
  repeated header/footer lines, markdown fence wrappers left by some VLMs,
  excessive blank lines).

#### CLI (`pdf2md`)
- All library options exposed as flags; `--inspect-only`, `--json`, `--metadata`.
- Detailed `--help` with model comparison table and cost estimates.
- Structured logging via `RUST_LOG` / `tracing-subscriber`.

#### Providers supported (via `edgequake-llm 0.2.2`)
| Provider | Example models |
|----------|---------------|
| OpenAI | `gpt-4.1-nano` (default), `gpt-4.1-mini`, `gpt-4.1`, `gpt-4o` |
| Anthropic | `claude-sonnet-4-20250514`, `claude-haiku-4-20250514` |
| Google Gemini | `gemini-2.0-flash`, `gemini-2.5-pro` |
| Ollama (local) | `llava`, `llama3.2-vision` |
| Azure OpenAI | via `AZURE_OPENAI_*` env vars |

#### Testing
- 21 unit tests covering config validation, post-processing rules, page
  selection logic, and provider resolution.
- 13 end-to-end integration tests (PDF round-trip, URL download, inspect,
  streaming) gated against a live API key (skipped in CI without one).
- 2 documentation tests.

#### Documentation
- Crate-level docs in `src/lib.rs` with pipeline diagram, model comparison
  table, and feature-flag reference.
- WHY-focused module and field documentation throughout the source.
- `docs/` directory: architecture, API guide, configuration reference, CLI
  reference, VLM provider guide.

### Dependencies
- `edgequake-llm 0.2.2` — fixes `OpenAIProvider::convert_messages()` silently
  dropping `ChatMessage.images` (multipart content array now correctly
  serialised). This is the **critical** fix that makes vision calls work with
  OpenAI and Azure OpenAI endpoints.
- `pdfium-render 0.8` — safe Rust bindings to the Chromium pdfium library.
- `tokio 1` (full features) — async runtime.
- `clap 4` (derive, `cli` feature only) — CLI argument parsing.
- `serde / serde_json` — serialisation for JSON output mode.
- `base64 0.22` — PNG → Base64 encoding for image API payloads.
- `tracing / tracing-subscriber` — structured, levelled logging.
- `anyhow / thiserror` — ergonomic error handling.

---

[Unreleased]: https://github.com/raphaelmansuy/edgequake-pdf2md/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/raphaelmansuy/edgequake-pdf2md/releases/tag/v0.1.0
