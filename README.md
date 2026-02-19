<p align="center">
  <h1 align="center">edgequake-pdf2md</h1>
  <p align="center"><strong>Convert PDF documents to clean Markdown using Vision Language Models</strong></p>
</p>

<p align="center">
  <a href="https://crates.io/crates/edgequake-pdf2md"><img src="https://img.shields.io/crates/v/edgequake-pdf2md.svg" alt="crates.io"></a>
  <a href="https://docs.rs/edgequake-pdf2md"><img src="https://docs.rs/edgequake-pdf2md/badge.svg" alt="docs.rs"></a>
  <a href="#license"><img src="https://img.shields.io/badge/license-Apache--2.0-blue.svg" alt="License"></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/rust-1.80%2B-orange.svg" alt="Rust 1.80+"></a>
</p>

---

`edgequake-pdf2md` is a Rust CLI and library that converts PDF files (local or URL) into well-structured Markdown using vision-capable LLMs. It rasterises each page with [pdfium](https://pdfium.googlesource.com/pdfium/), sends the image to a VLM (GPT-4.1, Claude, Gemini, etc.), and post-processes the result into clean Markdown.

Inspired by [pyzerox](https://github.com/getomni-ai/zerox), rebuilt in Rust for speed and reliability.

## Features

- **Multi-provider** — OpenAI, Anthropic, Google Gemini, Azure, Ollama, or any OpenAI-compatible endpoint
- **Fast** — concurrent page processing with configurable parallelism
- **Accurate** — 10-rule post-processing pipeline fixes tables, removes hallucinations, normalises output
- **Flexible** — page selection, fidelity tiers, custom system prompts, streaming API
- **Cross-platform** — macOS (ARM/x64), Linux (x64/ARM64/musl), Windows
- **Library + CLI** — use as a Rust crate or standalone command-line tool

## Quick Start

### 1. Install pdfium

```bash
# Auto-detect OS & architecture (recommended)
./scripts/setup-pdfium.sh

# macOS: set library path
export DYLD_LIBRARY_PATH="$(pwd)"

# Linux: set library path
export LD_LIBRARY_PATH="$(pwd)"
```

> **Note:** pdfium doesn't have an official Homebrew package. Use the setup script above or see [docs/installation.md](docs/installation.md) for manual installation options.

### 2. Set an API key

```bash
export OPENAI_API_KEY="sk-..."    # OpenAI (recommended)
# or
export ANTHROPIC_API_KEY="sk-ant-..."  # Anthropic
# or
export GEMINI_API_KEY="AI..."          # Google Gemini
```

### 3. Build & run

```bash
cargo build --release --features cli

# Convert a PDF
./target/release/pdf2md document.pdf -o output.md

# Convert from URL
./target/release/pdf2md https://arxiv.org/pdf/1706.03762 -o paper.md

# Inspect metadata (no API key needed)
./target/release/pdf2md --inspect-only document.pdf
```

Or install globally:
```bash
cargo install --path . --features cli
pdf2md document.pdf -o output.md
```

## How It Works

```
PDF ──▶ pdfium ──▶ PNG images ──▶ base64 ──▶ VLM API ──▶ post-process ──▶ Markdown
        render      per page       encode     (concurrent)   10 rules       assembled
```

1. **Input** — resolve local file or download from URL
2. **Render** — rasterise pages to images via [pdfium-render](https://crates.io/crates/pdfium-render)
3. **Encode** — base64-encode each page image
4. **VLM** — send images to a vision LLM with a structured system prompt
5. **Post-process** — strip fences, fix tables, remove hallucinated images, normalise whitespace
6. **Assemble** — join pages with optional separators and YAML front-matter

See [docs/how-it-works.md](docs/how-it-works.md) for the full pipeline walkthrough with diagrams.

## Usage

```bash
# Basic conversion
pdf2md document.pdf -o output.md

# Specific pages
pdf2md --pages 1-5 document.pdf -o first_five.md

# High fidelity with a better model
pdf2md --fidelity tier3 --model gpt-4.1 --dpi 200 paper.pdf -o paper.md

# Consistent formatting across pages (sequential mode)
pdf2md --maintain-format --separator hr book.pdf -o book.md

# JSON output with metadata
pdf2md --json --metadata document.pdf > output.json

# Use Anthropic
pdf2md --provider anthropic --model claude-sonnet-4-20250514 document.pdf

# Use local Ollama
pdf2md --provider ollama --model llava document.pdf
```

Run `pdf2md --help` for the full reference, including supported models and cost estimates.

See [docs/examples.md](docs/examples.md) for more usage patterns.

## Supported Providers & Models

| Provider | Model | Input $/1M | Output $/1M | Vision |
|----------|-------|-----------|-------------|--------|
| **OpenAI** | gpt-4.1-nano *(default)* | $0.10 | $0.40 | ✓ |
| **OpenAI** | gpt-4.1-mini | $0.40 | $1.60 | ✓ |
| **OpenAI** | gpt-4.1 | $2.00 | $8.00 | ✓ |
| **Anthropic** | claude-sonnet-4-20250514 | $3.00 | $15.00 | ✓ |
| **Anthropic** | claude-haiku-4-20250514 | $0.80 | $4.00 | ✓ |
| **Gemini** | gemini-2.0-flash | $0.10 | $0.40 | ✓ |
| **Gemini** | gemini-2.5-pro | $1.25 | $10.00 | ✓ |
| **Ollama** | llava, llama3.2-vision | free | free | ✓ |

**Cost estimate:** A 50-page document costs ~$0.02 with gpt-4.1-nano, ~$0.09 with gpt-4.1-mini.

See [docs/providers.md](docs/providers.md) for detailed comparisons, cost calculators, and selection guide.

## Library Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
edgequake-pdf2md = "0.2"
tokio = { version = "1", features = ["full"] }
```

### Basic conversion

```rust
use edgequake_pdf2md::{convert, ConversionConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ConversionConfig::builder()
        .model("gpt-4.1-nano")
        .provider_name("openai")
        .pages(edgequake_pdf2md::PageSelection::Range(1, 5))
        .build()?;

    let output = convert("document.pdf", &config).await?;
    println!("{}", output.markdown);
    println!("Processed {}/{} pages", output.stats.processed_pages, output.stats.total_pages);
    Ok(())
}
```

### Convert PDF bytes in memory *(v0.2)*

No temp-file management needed — pass raw bytes directly:

```rust
use edgequake_pdf2md::{convert_from_bytes, ConversionConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bytes = std::fs::read("document.pdf")?;  // or from DB / network
    let config = ConversionConfig::default();
    let output = convert_from_bytes(&bytes, &config).await?;
    println!("{}", output.markdown);
    Ok(())
}
```

### Per-page progress callbacks *(v0.2)*

```rust
use edgequake_pdf2md::{convert, ConversionConfig, ConversionProgressCallback};
use std::sync::Arc;

struct MyProgress;

impl ConversionProgressCallback for MyProgress {
    fn on_conversion_start(&self, total: usize) {
        eprintln!("Starting conversion of {total} pages");
    }
    fn on_page_complete(&self, page: usize, total: usize, chars: usize) {
        eprintln!("  ✓ Page {page}/{total} — {chars} chars");
    }
    fn on_page_error(&self, page: usize, total: usize, error: &str) {
        eprintln!("  ✗ Page {page}/{total} failed: {error}");
    }
    fn on_conversion_complete(&self, total: usize, success: usize) {
        eprintln!("Done: {success}/{total} pages converted");
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ConversionConfig::builder()
        .progress_callback(Arc::new(MyProgress) as Arc<dyn ConversionProgressCallback>)
        .build()?;
    let output = convert("document.pdf", &config).await?;
    println!("{}", output.markdown);
    Ok(())
}
```

### Strict error on partial failure *(v0.2)*

By default, page failures are non-fatal. Use `into_result()` to promote them to errors:

```rust
use edgequake_pdf2md::{convert, ConversionConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ConversionConfig::default();
    // into_result() returns Err(PartialFailure) if any pages failed
    let output = convert("document.pdf", &config).await?.into_result()?;
    println!("{}", output.markdown);
    Ok(())
}
```

### Provider injection *(v0.2)*

Pass a pre-built `Arc<dyn LLMProvider>` directly — useful for sharing providers
across multiple conversions and for testing with mocks:

```rust
use edgequake_pdf2md::{convert, ConversionConfig};
use edgequake_llm::ProviderFactory;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (provider, _) = ProviderFactory::from_env()?;
    let config = ConversionConfig::builder()
        .provider(Arc::clone(&provider))   // injected; highest priority
        .build()?;
    let output = convert("document.pdf", &config).await?;
    println!("{}", output.markdown);
    Ok(())
}
```

Provider resolution order (highest-to-lowest priority):
1. `config.provider` — explicit `Arc<dyn LLMProvider>` injection
2. `config.provider_name` + `config.model` — named provider
3. `EDGEQUAKE_LLM_PROVIDER` + `EDGEQUAKE_MODEL` environment variables
4. Auto-detect from API key env vars (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, …)

Also available: streaming API (`convert_stream`, `convert_stream_from_bytes`), sync wrapper (`convert_sync`), metadata inspection (`inspect`).

See [API docs on docs.rs](https://docs.rs/edgequake-pdf2md) for the full API reference.

## Configuration

All options can be set via CLI flags, environment variables, or the builder API:

| Flag | Env Variable | Default | Description |
|------|-------------|---------|-------------|
| `--model` | `EDGEQUAKE_MODEL` | gpt-4.1-nano | VLM model |
| `--provider` | `EDGEQUAKE_PROVIDER` | auto-detect | LLM provider |
| `--dpi` | `PDF2MD_DPI` | 150 | Rendering resolution (72–400) |
| `--pages` | `PDF2MD_PAGES` | all | Page selection |
| `--fidelity` | `PDF2MD_FIDELITY` | tier2 | Quality tier (tier1/tier2/tier3) |
| `-c, --concurrency` | `PDF2MD_CONCURRENCY` | 10 | Parallel API calls |
| `--maintain-format` | `PDF2MD_MAINTAIN_FORMAT` | false | Sequential mode |
| `--separator` | `PDF2MD_SEPARATOR` | none | Page separator |
| `--temperature` | `PDF2MD_TEMPERATURE` | 0.1 | LLM temperature |

See [docs/configuration.md](docs/configuration.md) for the complete reference.

## Development

```bash
# Setup
make setup          # Check pdfium + API key

# Build
make build          # Release binary
make build-dev      # Debug binary

# Test
make test           # Unit tests (no API key needed)
make test-e2e       # Integration tests (needs API key)
make test-all       # All tests

# Quality
make lint           # Clippy
make fmt            # Format code
make ci             # format + lint + unit tests

# Try it
make demo           # Convert sample page
make inspect-all    # Inspect test PDFs
```

## Documentation

| Document | Description |
|----------|-------------|
| [docs/how-it-works.md](docs/how-it-works.md) | Pipeline architecture with ASCII diagrams |
| [docs/installation.md](docs/installation.md) | Setup guide for all platforms |
| [docs/providers.md](docs/providers.md) | Supported models, pricing, selection guide |
| [docs/configuration.md](docs/configuration.md) | All CLI flags and environment variables |
| [docs/examples.md](docs/examples.md) | Real-world usage examples |

## Dependencies

| Crate | Purpose |
|-------|---------|
| [pdfium-render](https://crates.io/crates/pdfium-render) | PDF rasterisation via Google's pdfium C++ library |
| [edgequake-llm](https://crates.io/crates/edgequake-llm) | Multi-provider LLM abstraction (OpenAI, Anthropic, Gemini, etc.) |
| [tokio](https://crates.io/crates/tokio) | Async runtime |
| [image](https://crates.io/crates/image) | Image encoding (PNG/JPEG) |
| [clap](https://crates.io/crates/clap) | CLI argument parsing |

## External References

- [pdfium](https://pdfium.googlesource.com/pdfium/) — Google's open-source PDF rendering engine
- [pdfium-binaries](https://github.com/bblanchon/pdfium-binaries) — Pre-built pdfium binaries for all platforms
- [pyzerox](https://github.com/getomni-ai/zerox) — The Python project that inspired this tool
- [OpenAI Vision API](https://platform.openai.com/docs/guides/vision) — Image understanding with GPT-4.1
- [Anthropic Vision](https://docs.anthropic.com/en/docs/build-with-claude/vision) — Image understanding with Claude
- [Google Gemini](https://ai.google.dev/gemini-api/docs/vision) — Vision capabilities

## License

Copyright 2026 Raphaël MANSUY

Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with the License. You may obtain a copy of the License at

> <https://www.apache.org/licenses/LICENSE-2.0>

Unless required by applicable law or agreed to in writing, software distributed under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied. See the License for the specific language governing permissions and limitations under the License.

See [LICENSE](LICENSE) for the full text.
