# Installation Guide

## Prerequisites

- **Rust** ≥ 1.80 (for building from source)
- **libpdfium** — the Google Chromium PDF rendering library
- An **LLM API key** (OpenAI, Anthropic, Google Gemini, or a local Ollama instance)

## Step 1: Install libpdfium

`edgequake-pdf2md` uses [pdfium](https://pdfium.googlesource.com/pdfium/) to rasterise PDF pages. The library must be available at runtime.

### Automatic (recommended)

Run the bundled setup script — it detects your OS and architecture automatically:

```bash
./scripts/setup-pdfium.sh
```

This downloads the correct binary from [bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries) and places it in the current directory.

### macOS

**Option A — Setup script:**
```bash
./scripts/setup-pdfium.sh
export DYLD_LIBRARY_PATH="$(pwd)"
```

**Option B — Homebrew:**
```bash
brew install pdfium-chromium
```

**Option C — Manual download:**
```bash
# Apple Silicon (M1/M2/M3/M4)
curl -fSL "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F7690/pdfium-mac-arm64.tgz" -o /tmp/pdfium.tgz

# Intel Mac
curl -fSL "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F7690/pdfium-mac-x64.tgz" -o /tmp/pdfium.tgz

tar -xzf /tmp/pdfium.tgz -C /tmp lib/libpdfium.dylib
mv /tmp/lib/libpdfium.dylib .
export DYLD_LIBRARY_PATH="$(pwd)"
```

### Linux

**Option A — Setup script:**
```bash
./scripts/setup-pdfium.sh
export LD_LIBRARY_PATH="$(pwd)"
```

**Option B — Manual download:**
```bash
# x86_64
curl -fSL "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F7690/pdfium-linux-x64.tgz" -o /tmp/pdfium.tgz

# ARM64 (Raspberry Pi 4, AWS Graviton)
curl -fSL "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F7690/pdfium-linux-arm64.tgz" -o /tmp/pdfium.tgz

# Alpine Linux / musl
curl -fSL "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F7690/pdfium-linux-musl-x64.tgz" -o /tmp/pdfium.tgz

tar -xzf /tmp/pdfium.tgz -C /tmp lib/libpdfium.so
mv /tmp/lib/libpdfium.so .
export LD_LIBRARY_PATH="$(pwd)"
```

**Option C — System-wide install:**
```bash
sudo mv libpdfium.so /usr/local/lib/
sudo ldconfig
```

### Windows

```powershell
# Download (PowerShell)
Invoke-WebRequest -Uri "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F7690/pdfium-win-x64.tgz" -OutFile pdfium.tgz

# Extract
tar -xzf pdfium.tgz bin/pdfium.dll
Move-Item bin\pdfium.dll .
```

Place `pdfium.dll` in the same directory as `pdf2md.exe`, or add its directory to your `PATH`.

## Step 2: Build pdf2md

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

## Step 3: Set Up an LLM Provider

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

## Step 4: Verify

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

The pdfium native library cannot be found. Solutions:

1. Run `./scripts/setup-pdfium.sh` to auto-download
2. Set `DYLD_LIBRARY_PATH` (macOS) or `LD_LIBRARY_PATH` (Linux) to the directory containing the library
3. Place the library next to the `pdf2md` binary

### "No LLM provider could be auto-detected"

No API key environment variable is set. Export at least one:
- `OPENAI_API_KEY`
- `ANTHROPIC_API_KEY`
- `GEMINI_API_KEY`

### "LLM API error: 401 Unauthorized"

Your API key is invalid or expired. Generate a new one from your provider's dashboard.

### macOS: "dyld: Library not loaded"

You need to set the dynamic library path:
```bash
export DYLD_LIBRARY_PATH="/path/to/directory/with/libpdfium.dylib"
```
