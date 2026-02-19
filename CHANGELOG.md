# Changelog

All notable changes to `edgequake-pdf2md` will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

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
