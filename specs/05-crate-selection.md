# 05 — Crate Selection

> **See also**: [Index](./00-index.md) · [Algorithm](./02-algorithm.md) · [API Design](./06-api-design.md)

---

This document evaluates all Rust crates required for the implementation, documents the selection criteria, and records the final decisions with rationale.

---

## 1. PDF Rendering

### Candidates

| Crate | Strategy | License | Stars | Downloads |
|-------|----------|---------|-------|-----------|
| **pdfium-render** | libpdfium (Chromium's PDF engine) | MIT/Apache | ★★★★ | 742K |
| pdf2image | wraps `pdftoppm` (poppler) binary | MIT | ★★ | 30K |
| poppler-rs | unsound bindings to libpoppler | LGPL | ★ | low |
| mupdf-sys | libmupdf binding | AGPL-3 | ★★ | 50K |
| lopdf | pure Rust, text extraction only | MIT | ★★★ | 1.2M |
| pdf-extract | pure Rust, text extraction only | MIT | ★★ | 200K |
| resvg/pdfium | N/A (SVG not PDF) | MPL-2 | N/A | N/A |

### Evaluation Criteria

```
Weight  Criterion
──────  ─────────────────────────────────────────────────────
  5     Rendering quality (visual fidelity vs. Acrobat)
  4     No external binary dependency at runtime
  4     Active maintenance (last release < 6 months)
  3     Cross-platform (macOS, Linux, Windows)
  3     Thread safety / async compatibility
  2     Static linking possible (self-contained binary)
  2     WASM potential (future)
  1     Pure Rust (no FFI)
  1     License permissiveness
```

### Detailed Analysis

#### pdfium-render (CHOSEN ✓)

```
crates.io: https://crates.io/crates/pdfium-render
docs.rs:   https://docs.rs/pdfium-render
github:    https://github.com/ajrcarey/pdfium-render
version:   0.8.37 (Jan 2026)

Pros:
  ✓ Wraps Google's Pdfium — the same engine used by:
      Chrome, Chromium, Android WebView
      i.e. matches what users see in their browser
  ✓ Excellent rendering quality (transparency, blending, CFF fonts)
  ✓ Idiomatic Rust API (not just raw FFI)
  ✓ Thread safe with `thread_safe` feature (mutex-backed)
  ✓ Static linking supported (supply your own libpdfium.a)
  ✓ Dynamic linking with pre-built binaries from bblanchon/pdfium-binaries
  ✓ WASM support (separate WASM build of pdfium)
  ✓ Actively maintained: 84 versions, last update within 3 months
  ✓ MIT OR Apache-2.0 dual license
  ✓ No runtime external binary (links libpdfium directly)
  ✓ `spawn_blocking` compatible for CPU-bound rasterisation

Cons:
  ✗ FFI boundary (not pure Rust; inherits C++ complexity)
  ✗ Must distribute or link libpdfium (~30MB)
  ✗ Pdfium not included in crate; must be provided separately
  ✗ MSRV 1.61 (1.80.1 with image feature)

Mitigation for libpdfium distribution:
  Dynamic: user installs via package manager or bundled alongside binary
  Static:  build.rs downloads pdfium-binaries/releases for target platform
```

#### pdf2image

```
Pros: Simple API, familiar (Python's pdf2image equivalent)
Cons: 
  ✗ Requires poppler-utils installed as external binary
  ✗ Lower rendering quality (poppler vs. pdfium)
  ✗ Last release 1 year ago (less maintained)
  ✗ Only 4 versions, 30K downloads
VERDICT: Rejected — external binary dependency, lower quality
```

#### mupdf-sys

```
Pros: Excellent rendering quality, widely used
Cons:
  ✗ AGPL-3 license → copyleft contamination for commercial use
  ✗ More complex bindings
VERDICT: Rejected — license conflict
```

#### lopdf / pdf-extract

```
Pros: Pure Rust, no native dependency
Cons:
  ✗ Text extraction only, NO rendering
  ✗ Fails on scanned PDFs entirely
  ✗ Fails on CID fonts, CFF fonts
VERDICT: Not applicable to our use case
```

**Decision: `pdfium-render = "0.8"`**

```toml
[dependencies]
pdfium-render = { version = "0.8", features = ["pdfium_latest", "image_latest", "thread_safe"] }
```

---

## 2. LLM Providers

**Decision: `edgequake-llm`** (required)

```toml
[dependencies]
edgequake-llm = "0.2"
```

This provides:
- `LLMProvider` trait
- `ChatMessage::user_with_images(prompt, vec![ImageData])`
- Providers: OpenAI, Anthropic, Azure, Gemini, Ollama, LM Studio, OpenRouter, xAI, HuggingFace, VSCode Copilot
- Built-in rate limiting, retry, caching, cost tracking
- `ProviderFactory::from_env()` for zero-code provider selection

No alternative considered — this is a first-party requirement.

---

## 3. Async Runtime

**Decision: `tokio`** (ubiquitous, required by edgequake-llm)

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
```

Key features used:
- `tokio::task::spawn_blocking` — for CPU-bound pdfium rasterisation
- `tokio::sync::Semaphore` — for concurrency limiting
- `tokio_stream::StreamExt` — for async streaming output
- `tokio::io` — async file I/O

---

## 4. HTTP Client (URL Download)

**Decision: `reqwest`**

```toml
[dependencies]
reqwest = { version = "0.12", features = ["stream", "rustls-tls"] }
```

Justification:
- `stream` feature: byte-range streaming download into tempfile (no full RAM load)
- `rustls-tls`: pure-Rust TLS, no OpenSSL dependency at compile time
- Follows redirects by default
- Respects `Content-Disposition` for filename extraction
- Already a transitive dependency of edgequake-llm (no duplication)

---

## 5. Image Processing

**Decision: `image`**

```toml
[dependencies]
image = { version = "0.25", default-features = false, features = ["png", "jpeg"] }
```

Used for:
- Converting `PdfBitmap` RGBA data to `DynamicImage`
- PNG encoding (`DynamicImage::into_rgb8()` → PNG bytes)
- Optional JPEG encoding for lower-quality / smaller base64 payloads
- Pixel dimension capping for very large pages

---

## 6. Base64 Encoding

**Decision: `base64`**

```toml
[dependencies]
base64 = "0.22"
```

Used for:
- `general_purpose::STANDARD.encode(png_bytes)` → base64 String for `ImageData`

---

## 7. CLI Framework

**Decision: `clap`**

```toml
[dependencies]
clap = { version = "4", features = ["derive", "env", "color", "wrap_help"] }
```

Justification:
- Industry standard for Rust CLIs
- `derive` feature: attribute-based arg parsing (most ergonomic)
- `env` feature: `#[arg(env = "PDF2MD_MODEL")]` auto-read from env vars
- `color`: coloured help text
- `wrap_help`: auto-wraps long help text

Alternative considered: `argh` — simpler but less featured; rejected.

---

## 8. Temporary File Management

**Decision: `tempfile`**

```toml
[dependencies]
tempfile = "3"
```

Used for:
- `tempfile::Builder::new()` → temp dir for downloaded PDFs
- Automatic cleanup on `TempDir` drop
- Safe temp path generation (no TOCTOU)

---

## 9. Error Handling

**Decision: `thiserror` + `anyhow`**

```toml
[dependencies]
thiserror = "2"  # For library error types (specific, typed)
anyhow    = "2"  # For binary/CLI error handling (simple, boxed)
```

Pattern:
- Library crate (`lib.rs`): use `thiserror` to define `Pdf2MdError` enum
- CLI binary (`main.rs`): use `anyhow::Result` for `main()` error propagation

---

## 10. Logging and Observability

**Decision: `tracing` + `tracing-subscriber`**

```toml
[dependencies]
tracing = "0.1"

[dev-dependencies]
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

Used in library for `tracing::info!`, `tracing::debug!`, `tracing::warn!`.
CLI configures subscriber with `RUST_LOG=pdf2md=info` env filter.

---

## 11. Async Streams

**Decision: `futures` + `tokio-stream`**

```toml
[dependencies]
futures     = "0.3"
tokio-stream = "0.1"
```

Used for:
- `futures::stream::iter(pages).buffer_unordered(concurrency)` — parallel page processing
- `tokio_stream::wrappers::ReceiverStream` — streaming output via mpsc channel

---

## 12. Serialisation (Stats, Config)

**Decision: `serde` + `serde_json`**

```toml
[dependencies]
serde      = { version = "1", features = ["derive"] }
serde_json = "1"
```

Used for:
- Serialisable `ConversionOutput` / `ConversionStats` for `--json` CLI output
- `ConversionConfig` serialisation for config file loading

---

## 13. Configuration File (Optional)

**Decision: `toml`**

```toml
[dependencies]
toml = { version = "0.8", optional = true }
```

Used for optional `pdf2md.toml` config file loading in CLI. Feature-gated to avoid compile-time bloat for library users.

---

## 14. Progress Display (CLI Only)

**Decision: `indicatif`**

```toml
[dependencies]
indicatif = { version = "0.17", optional = true }
```

Feature `progress` enables progress bars in the CLI:
```
Converting: [████████░░] 8/10 pages  • 2.3 tok/s  • elapsed: 24s
```

---

## 15. Complete `Cargo.toml` (Library + Binary)

```toml
[package]
name    = "edgequake-pdf2md"
version = "0.1.0"
edition = "2021"
rust-version = "1.80"
description  = "Convert PDF to Markdown using Vision LLM via edgequake-llm"
license      = "MIT OR Apache-2.0"
repository   = "https://github.com/your-org/edgequake-pdf2md"
keywords     = ["pdf", "markdown", "llm", "ocr", "vision"]
categories   = ["command-line-utilities", "text-processing"]

[[bin]]
name = "pdf2md"
path = "src/bin/pdf2md.rs"

[lib]
name = "edgequake_pdf2md"
path = "src/lib.rs"

[dependencies]
# Core
pdfium-render  = { version = "0.8", features = ["pdfium_latest", "image_latest", "thread_safe"] }
edgequake-llm  = "0.2"
tokio          = { version = "1", features = ["full"] }
futures        = "0.3"
tokio-stream   = "0.1"

# HTTP
reqwest        = { version = "0.12", features = ["stream", "rustls-tls"], default-features = false }

# Image
image          = { version = "0.25", default-features = false, features = ["png", "jpeg"] }
base64         = "0.22"

# File system
tempfile       = "3"

# Serialisation
serde          = { version = "1", features = ["derive"] }
serde_json     = "1"

# Error handling
thiserror      = "2"

# Logging
tracing        = "0.1"

# CLI (only for binary)
clap           = { version = "4", features = ["derive", "env", "color", "wrap_help"], optional = true }
indicatif      = { version = "0.17", optional = true }
anyhow         = { version = "2", optional = true }
tracing-subscriber = { version = "0.3", features = ["env-filter"], optional = true }
toml           = { version = "0.8", optional = true }

[features]
default = ["cli"]
cli     = ["dep:clap", "dep:anyhow", "dep:tracing-subscriber", "dep:indicatif", "dep:toml"]
progress = ["dep:indicatif"]

[dev-dependencies]
tokio-test        = "0.4"
tempfile          = "3"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

---

## 16. libpdfium Distribution Strategy

Since `pdfium-render` does not bundle pdfium itself, we provide:

### Option A: Dynamic linking (default for development)
```bash
# macOS
brew install pdfium  # or manual download from bblanchon/pdfium-binaries

# Linux
wget https://github.com/bblanchon/pdfium-binaries/releases/latest/...
export PDFIUM_DYNAMIC_LIB_PATH=/path/to/dir/containing/libpdfium.so
cargo build
```

### Option B: Static linking (for self-contained binary distribution)
```bash
# Download static pdfium from bblanchon/pdfium-binaries (release build)
export PDFIUM_STATIC_LIB_PATH=/path/to/dir/containing/libpdfium.a
cargo build --release
```

### Option C: Automated download via build.rs (recommended for CI)
A `build.rs` script automates downloading the correct pre-built pdfium binary for the current target platform from [bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries/releases). This ensures reproducible builds. See [07-cli-design.md §Distribution](./07-cli-design.md#distribution) for packaging details.

**Pdfium binary sizes** (compressed release archive):
- Linux x86_64: ~8MB
- macOS arm64: ~8MB
- macOS x86_64: ~9MB
- Windows x86_64: ~9MB

Total binary size estimate (stripped release build):
```
pdf2md binary:    ~5MB
libpdfium.so:     ~30MB (dynamic) or embedded (static)
Total (dynamic):  ~5MB + 30MB installed separately
Total (static):   ~35MB single binary
```
