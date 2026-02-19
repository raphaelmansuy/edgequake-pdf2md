# Changelog

All notable changes to `edgequake-pdf2md` will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

---

## [0.1.0] — 2026-02-19

### Added

#### Core library
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
