# Installation Guide

## Prerequisites

- **Rust** ≥ 1.80 (for building from source)
- An **LLM API key** (OpenAI, Anthropic, Google Gemini, or a local Ollama instance)

> **No manual PDFium setup required.** Starting from v0.3.0, `edgequake-pdf2md` automatically
> downloads the correct [pdfium](https://pdfium.googlesource.com/pdfium/) binary (~30 MB) from
> [bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries) on first run and
> caches it in `~/.cache/pdf2md/pdfium-7690/`. Subsequent runs use the cached copy with no
> network access.

## Step 1: Build pdf2md

```bash
# Clone the repository
git clone https://github.com/raphaelmansuy/edgequake-pdf2md.git
cd edgequake-pdf2md

# Build release binary
cargo build --release --features cli

# Or install to ~/.cargo/bin
cargo install --path . --features cli
```

The binary is at `target/release/pdf2md`.

## Step 2: Set Up an LLM Provider

Set at least one API key:

```bash
# OpenAI (recommended — best cost/quality ratio with gpt-4.1-nano)
export OPENAI_API_KEY="sk-..."

# Anthropic
export ANTHROPIC_API_KEY="sk-ant-..."

# Google Gemini
export GEMINI_API_KEY="AI..."
```

### Using Ollama (free, local)

```bash
# Install Ollama: https://ollama.com
ollama pull llava

# Run pdf2md with Ollama
pdf2md --provider ollama --model llava document.pdf
```

## Step 3: Verify

```bash
# Check pdfium is found
pdf2md --inspect-only some-document.pdf

# Convert a page
pdf2md --pages 1 document.pdf
```

## Quick Start with Make

The project includes a `Makefile` for developer convenience:

```bash
make setup     # Check pdfium + API key
make build     # Build release binary
make demo      # Convert a sample page
make test      # Run unit tests
make help      # Show all targets
```

## Docker (Coming Soon)

A Docker image with pdfium pre-installed is planned. See the project README for updates.

## Troubleshooting

### "Failed to bind to pdfium library"

PDFium is downloaded automatically on first run. If auto-download fails:

1. Check your internet connection and try again.
2. Set `PDFIUM_LIB_PATH=/path/to/libpdfium` to point to an existing copy.
3. Override the cache directory with `PDFIUM_AUTO_CACHE_DIR=/your/dir`.

### "No LLM provider could be auto-detected"

No API key environment variable is set. Export at least one:
- `OPENAI_API_KEY`
- `ANTHROPIC_API_KEY`
- `GEMINI_API_KEY`

### "LLM API error: 401 Unauthorized"

Your API key is invalid or expired. Generate a new one from your provider's dashboard.

### macOS: "dyld: Library not loaded"

This should not happen with v0.3.0+ because pdfium is loaded from its absolute cached path
(`~/.cache/pdf2md/pdfium-7690/libpdfium.dylib`). If you see this with an older version:
```bash
export PDFIUM_LIB_PATH="~/.cache/pdf2md/pdfium-7690/libpdfium.dylib"
```

### Using an existing pdfium installation

To skip the auto-download and use a library already on your system:
```bash
export PDFIUM_LIB_PATH="/usr/local/lib/libpdfium.dylib"  # macOS
export PDFIUM_LIB_PATH="/usr/local/lib/libpdfium.so"     # Linux
```

This also accepts the path from the legacy `./scripts/setup-pdfium.sh` script.
