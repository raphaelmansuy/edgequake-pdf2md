# 07 — CLI Design

> **See also**: [Index](./00-index.md) · [API Design](./06-api-design.md) · [Crate Selection](./05-crate-selection.md) · [Error Handling](./08-error-handling.md)

---

## 1. Binary Name and Purpose

```
pdf2md — Convert PDF files and URLs to Markdown using Vision LLMs
```

The binary is compiled from `src/bin/pdf2md.rs` and enabled by the `cli` feature (default-on).
It is a thin shim over the library crate that maps CLI flags to `ConversionConfig` and prints results.

---

## 2. Invocation Syntax

```
pdf2md [OPTIONS] <INPUT>
```

`<INPUT>` may be:
- A local file path: `./report.pdf`, `/tmp/doc.pdf`
- An HTTP/HTTPS URL: `https://arxiv.org/pdf/2309.12345`

---

## 3. Full Flag Reference

| Flag | Short | Env Var | Type | Default | Description |
|------|-------|---------|------|---------|-------------|
| `--output` | `-o` | `PDF2MD_OUTPUT` | path | stdout | Write Markdown to this file instead of stdout |
| `--model` | | `EDGEQUAKE_MODEL` | string | provider default | LLM model ID, e.g. `gpt-4o` |
| `--provider` | | `EDGEQUAKE_PROVIDER` | string | auto-detect from env | LLM provider name: `openai`, `anthropic`, `azure`, `gemini`, `ollama`, `lmstudio`, `openrouter`, `xai`, `huggingface`, `vscode-copilot`, `mock` |
| `--dpi` | | `PDF2MD_DPI` | u32 | `150` | Rendering resolution (72–400) |
| `--concurrency` | `-c` | `PDF2MD_CONCURRENCY` | usize | `10` | Parallel VLM API calls |
| `--maintain-format` | | `PDF2MD_MAINTAIN_FORMAT` | flag | false | Sequential mode; injects previous page as context |
| `--pages` | | `PDF2MD_PAGES` | string | all | Page selection: `all`, `5`, `3-15`, `1,3,5,7` |
| `--fidelity` | | `PDF2MD_FIDELITY` | string | `tier2` | Output quality: `tier1`, `tier2`, `tier3` |
| `--separator` | | `PDF2MD_SEPARATOR` | string | `none` | Page separator: `none`, `---`, `comment`, or literal string |
| `--password` | | `PDF2MD_PASSWORD` | string | none | PDF user password for encrypted documents |
| `--system-prompt` | | `PDF2MD_SYSTEM_PROMPT` | string | built-in | Path to text file containing custom system prompt |
| `--max-tokens` | | `PDF2MD_MAX_TOKENS` | u32 | `4096` | Max LLM output tokens per page |
| `--temperature` | | `PDF2MD_TEMPERATURE` | f32 | `0.1` | LLM temperature |
| `--max-retries` | | `PDF2MD_MAX_RETRIES` | u32 | `3` | Retries per page on LLM failure |
| `--metadata` | | `PDF2MD_METADATA` | flag | false | Prepend YAML front-matter with document metadata |
| `--json` | | `PDF2MD_JSON` | flag | false | Output structured JSON (`ConversionOutput`) instead of Markdown |
| `--no-progress` | | `PDF2MD_NO_PROGRESS` | flag | false | Disable progress bar (auto-disabled if stdout is not a TTY) |
| `--download-timeout` | | `PDF2MD_DOWNLOAD_TIMEOUT` | u64 | `120` | HTTP download timeout in seconds |
| `--api-timeout` | | `PDF2MD_API_TIMEOUT` | u64 | `60` | Per-page LLM call timeout in seconds |
| `--inspect` | | | flag | false | Print PDF metadata only, no conversion |
| `--verbose` | `-v` | `PDF2MD_VERBOSE` | flag | false | Enable DEBUG-level tracing logs |
| `--quiet` | `-q` | `PDF2MD_QUIET` | flag | false | Suppress all output except errors |
| `--version` | `-V` | | flag | | Print version and exit |
| `--help` | `-h` | | flag | | Print help and exit |

---

## 4. clap Configuration

```rust
#[derive(Parser, Debug)]
#[command(
    name = "pdf2md",
    version,
    about = "Convert PDF files and URLs to Markdown using Vision LLMs",
    long_about = None,
    arg_required_else_help = true,
    color = clap::ColorChoice::Auto,
    help_expected = true,
)]
pub struct Cli {
    /// Local PDF file path or HTTP/HTTPS URL
    pub input: String,

    #[arg(short, long, env = "PDF2MD_OUTPUT")]
    pub output: Option<PathBuf>,

    #[arg(long, env = "EDGEQUAKE_MODEL")]
    pub model: Option<String>,

    #[arg(long, env = "EDGEQUAKE_PROVIDER")]
    pub provider: Option<String>,

    #[arg(long, env = "PDF2MD_DPI", default_value_t = 150,
          value_parser = clap::value_parser!(u32).range(72..=400))]
    pub dpi: u32,

    #[arg(short, long, env = "PDF2MD_CONCURRENCY", default_value_t = 10)]
    pub concurrency: usize,

    #[arg(long, env = "PDF2MD_MAINTAIN_FORMAT")]
    pub maintain_format: bool,

    #[arg(long, env = "PDF2MD_PAGES", default_value = "all")]
    pub pages: String,

    #[arg(long, env = "PDF2MD_FIDELITY",
          value_enum, default_value = "tier2")]
    pub fidelity: FidelityArg,

    #[arg(long, env = "PDF2MD_SEPARATOR", default_value = "none")]
    pub separator: String,

    #[arg(long, env = "PDF2MD_PASSWORD")]
    pub password: Option<String>,

    #[arg(long, env = "PDF2MD_SYSTEM_PROMPT")]
    pub system_prompt: Option<PathBuf>,

    #[arg(long, env = "PDF2MD_MAX_TOKENS", default_value_t = 4096)]
    pub max_tokens: u32,

    #[arg(long, env = "PDF2MD_TEMPERATURE", default_value_t = 0.1)]
    pub temperature: f32,

    #[arg(long, env = "PDF2MD_MAX_RETRIES", default_value_t = 3)]
    pub max_retries: u32,

    #[arg(long, env = "PDF2MD_METADATA")]
    pub metadata: bool,

    #[arg(long, env = "PDF2MD_JSON")]
    pub json: bool,

    #[arg(long, env = "PDF2MD_NO_PROGRESS")]
    pub no_progress: bool,

    #[arg(long)]
    pub inspect: bool,

    #[arg(short, long, env = "PDF2MD_VERBOSE")]
    pub verbose: bool,

    #[arg(short, long, env = "PDF2MD_QUIET")]
    pub quiet: bool,
}

#[derive(clap::ValueEnum, Clone, Debug)]
pub enum FidelityArg { Tier1, Tier2, Tier3 }
```

**External reference**: [clap v4 derive docs](https://docs.rs/clap/latest/clap/_derive/index.html)

---

## 5. Page Selection Parsing

The `--pages` argument is parsed from a string into `PageSelection`:

| Input | Result |
|-------|--------|
| `all` | `PageSelection::All` |
| `5` | `PageSelection::Single(5)` |
| `3-15` | `PageSelection::Range(3..=15)` |
| `1,3,5,7` | `PageSelection::Set(vec![1,3,5,7])` |

The parser should produce clear errors:
```
error: invalid page selection '0': pages are 1-indexed, minimum is 1
error: invalid page range '15-3': start must be <= end
```

---

## 6. UX and Progress Bar

The progress bar is rendered using [`indicatif`](https://docs.rs/indicatif/latest/indicatif/) and shows:

```
Converting report.pdf   [=======--------] 14/50 pages  28%  ⏱ 00:00:32 ETA 00:01:10
Model: gpt-4o · DPI: 150 · Concurrency: 10
```

Behaviour:
- Displayed only when stdout is a TTY (`atty::is(atty::Stream::Stdout)`)
- Suppressed with `--no-progress` or `--quiet`
- Suppressed when output is piped: `pdf2md report.pdf | wc -c`
- Uses per-page completion ticks; increments on completion or failure
- Footer line shows model, DPI, concurrency (from resolved config)
- On failure: bar style changes to red; failed pages shown in summary

---

## 7. Output Modes

### 7.1 Default — Markdown to stdout

```
$ pdf2md report.pdf
# Introduction
...
```

### 7.2 Write to file

```
$ pdf2md report.pdf -o report.md
✓ Converted 50 pages in 41s → report.md
```

The filepath is written to **stderr** so that `> report.md` redirection still works.

### 7.3 JSON output

```
$ pdf2md report.pdf --json | jq .stats
{
  "total_pages": 50,
  "processed_pages": 50,
  "failed_pages": 0,
  "total_input_tokens": 38200,
  "total_output_tokens": 12400,
  "total_duration_ms": 41323
}
```

The full `ConversionOutput` struct (serialised with `serde_json::to_string_pretty`) is emitted.

### 7.4 Inspect mode

```
$ pdf2md --inspect research_paper.pdf
PDF Version: 1.7
Title:       "Attention Is All You Need"
Author:      "Vaswani et al."
Pages:       15
Encrypted:   false
Linearised:  true
Producer:    "LaTeX with hyperref"
Created:     2017-06-12T00:00:00Z
```

---

## 8. Tracing / Logging

```rust
// bin/pdf2md.rs
let level = if cli.verbose { "debug" } else { "warn" };
tracing_subscriber::fmt()
    .with_env_filter(format!("edgequake_pdf2md={level},warn"))
    .with_writer(std::io::stderr)
    .init();
```

- Logs go to **stderr** (never pollute stdout Markdown output)
- `--verbose` enables DEBUG level for the crate
- `RUST_LOG` env var overrides entirely (standard `tracing_subscriber` behaviour)
- **External reference**: [tracing-subscriber docs](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/)

---

## 9. Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Success — all pages converted |
| `1` | Fatal error — conversion not attempted (bad input, no API key, corrupt PDF) |
| `2` | Partial success — at least one page failed |
| `3` | Configuration error (bad arguments, invalid flag values) |

---

## 10. Environment Variable Precedence

```
CLI flag  >  environment variable  >  default value
```

This follows clap's standard `env()` behaviour. Example:

```
PDF2MD_DPI=300 pdf2md --dpi 150 report.pdf
```

→ uses DPI 150 (CLI flag wins).

---

## 11. Example Invocations

```bash
# Simple conversion
pdf2md report.pdf

# Write to file with higher DPI
pdf2md report.pdf -o output.md --dpi 200

# Convert from URL using Anthropic Claude
ANTHROPIC_API_KEY=sk-... pdf2md --provider anthropic \
    --model claude-3-5-sonnet-20241022 \
    https://arxiv.org/pdf/2310.07093

# Local LLM via Ollama (slow, private)
pdf2md --provider ollama --model llava:34b -c 1 private.pdf

# Selected pages as JSON
pdf2md --pages 3-7 --json report.pdf | jq .pages[].markdown

# Encrypted PDF
pdf2md --password s3cr3t encrypted.pdf -o decrypted.md

# Maintain format (sequential, best for books)
pdf2md --maintain-format --separator --- novel.pdf

# Inspect without converting
pdf2md --inspect report.pdf

# High-fidelity with LaTeX math
pdf2md --fidelity tier3 --model gpt-4o math_paper.pdf

# CI/CD — no progress bar, warn only
PDF2MD_NO_PROGRESS=1 PDF2MD_QUIET=1 pdf2md report.pdf -o output.md
```

---

## 12. Distribution

### Cargo install

```
cargo install edgequake-pdf2md
```

This compiles the `cli` feature (default). Users must separately place `libpdfium` in `LD_LIBRARY_PATH` (Linux), `DYLD_LIBRARY_PATH` (macOS), or `PATH` (Windows DLL).

### Static linking (recommended for release builds)

Enable the `pdfium-static` feature:
```
cargo install edgequake-pdf2md --features pdfium-static
```

Bundles pdfium into the binary (~35MB). No external library needed.

### Pre-built release binaries

Release CI builds produce:
- `pdf2md-x86_64-apple-darwin.tar.gz`
- `pdf2md-aarch64-apple-darwin.tar.gz`
- `pdf2md-x86_64-unknown-linux-gnu.tar.gz`
- `pdf2md-x86_64-pc-windows-msvc.zip`

Each archive contains the `pdf2md` binary and the matching `libpdfium` `.so`/`.dylib`/`.dll`.

**External reference**: [pdfium-binaries releases](https://github.com/bblanchon/pdfium-binaries/releases)
