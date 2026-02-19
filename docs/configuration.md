# Configuration Reference

Complete reference for all CLI flags, environment variables, and library configuration options.

## CLI Flags

### Input & Output

| Flag | Env Variable | Default | Description |
|------|-------------|---------|-------------|
| `<INPUT>` | — | (required) | PDF file path or HTTP/HTTPS URL |
| `-o, --output <PATH>` | `PDF2MD_OUTPUT` | stdout | Write markdown to a file |
| `--json` | `PDF2MD_JSON` | false | Output structured JSON instead of markdown |
| `--metadata` | `PDF2MD_METADATA` | false | Include YAML front-matter with document metadata |
| `--inspect-only` | — | false | Print PDF metadata only (no LLM needed) |

### Model & Provider

| Flag | Env Variable | Default | Description |
|------|-------------|---------|-------------|
| `--model <ID>` | `EDGEQUAKE_MODEL` | gpt-4.1-nano | VLM model identifier |
| `--provider <NAME>` | `EDGEQUAKE_PROVIDER` | auto-detect | LLM provider name |

### Rendering

| Flag | Env Variable | Default | Range | Description |
|------|-------------|---------|-------|-------------|
| `--dpi <N>` | `PDF2MD_DPI` | 150 | 72–400 | Rendering resolution |
| `--pages <SPEC>` | `PDF2MD_PAGES` | all | — | Page selection |
| `--fidelity <TIER>` | `PDF2MD_FIDELITY` | tier2 | tier1/tier2/tier3 | Output quality tier |

### Processing

| Flag | Env Variable | Default | Description |
|------|-------------|---------|-------------|
| `-c, --concurrency <N>` | `PDF2MD_CONCURRENCY` | 10 | Max concurrent VLM calls |
| `--maintain-format` | `PDF2MD_MAINTAIN_FORMAT` | false | Sequential mode for format continuity |
| `--separator <TYPE>` | `PDF2MD_SEPARATOR` | none | Page separator: none, hr, comment, or custom |
| `--password <PWD>` | `PDF2MD_PASSWORD` | — | PDF decrypt password |
| `--system-prompt <FILE>` | `PDF2MD_SYSTEM_PROMPT` | built-in | Custom system prompt file |

### Tuning

| Flag | Env Variable | Default | Description |
|------|-------------|---------|-------------|
| `--max-tokens <N>` | `PDF2MD_MAX_TOKENS` | 4096 | Max output tokens per page |
| `--temperature <F>` | `PDF2MD_TEMPERATURE` | 0.1 | LLM temperature (0.0–2.0) |
| `--max-retries <N>` | `PDF2MD_MAX_RETRIES` | 3 | Retries per page on LLM failure |
| `--download-timeout <S>` | `PDF2MD_DOWNLOAD_TIMEOUT` | 120 | HTTP download timeout (seconds) |
| `--api-timeout <S>` | `PDF2MD_API_TIMEOUT` | 60 | Per-page LLM timeout (seconds) |

### Output Control

| Flag | Env Variable | Default | Description |
|------|-------------|---------|-------------|
| `-v, --verbose` | `PDF2MD_VERBOSE` | false | Enable DEBUG-level logs |
| `-q, --quiet` | `PDF2MD_QUIET` | false | Suppress all output except errors |
| `--no-progress` | `PDF2MD_NO_PROGRESS` | false | Disable progress bar |

## Page Selection Syntax

| Syntax | Description | Example |
|--------|-------------|---------|
| `all` | All pages (default) | `--pages all` |
| `N` | Single page (1-indexed) | `--pages 5` |
| `M-N` | Range (inclusive) | `--pages 3-15` |
| `A,B,C` | Specific set | `--pages 1,3,5,7` |

## Environment Variables

### LLM API Keys

| Variable | Provider |
|----------|----------|
| `OPENAI_API_KEY` | OpenAI |
| `ANTHROPIC_API_KEY` | Anthropic |
| `GEMINI_API_KEY` | Google Gemini |
| `AZURE_OPENAI_API_KEY` | Azure OpenAI |

### Library Path

| Variable | Platform | Purpose |
|----------|----------|---------|
| `DYLD_LIBRARY_PATH` | macOS | Path to `libpdfium.dylib` |
| `LD_LIBRARY_PATH` | Linux | Path to `libpdfium.so` |
| `PATH` | Windows | Directory containing `pdfium.dll` |

### Control Variables

| Variable | Description |
|----------|-------------|
| `EDGEQUAKE_LLM_PROVIDER` | Override provider (openai, anthropic, gemini, ollama) |
| `EDGEQUAKE_MODEL` | Override model ID |
| `PDFIUM_DYNAMIC_LIB_PATH` | Compile-time path to pdfium library |
| `RUST_LOG` | Tracing filter (e.g., `debug`, `edgequake_pdf2md=trace`) |

## Library API Configuration

When using `edgequake-pdf2md` as a Rust library:

```rust
use edgequake_pdf2md::{ConversionConfig, FidelityTier, PageSelection, PageSeparator};

let config = ConversionConfig::builder()
    .dpi(200)                              // Higher resolution
    .concurrency(5)                        // Fewer concurrent calls
    .model("gpt-4.1")                     // Specific model
    .provider_name("openai")              // Specific provider
    .temperature(0.0)                     // Deterministic output
    .max_tokens(8192)                     // Longer page output
    .max_retries(5)                       // More retries
    .fidelity(FidelityTier::Tier3)        // Highest quality
    .pages(PageSelection::Range(1, 10))   // Pages 1–10
    .page_separator(PageSeparator::HorizontalRule)
    .include_metadata(true)               // YAML front-matter
    .maintain_format(true)                // Sequential processing
    .download_timeout_secs(300)           // Longer download timeout
    .api_timeout_secs(120)                // Longer API timeout
    .build()
    .expect("Invalid config");
```

## Configuration Precedence

Provider resolution follows this order (first match wins):

```
1. config.provider        (pre-built Arc<dyn LLMProvider>)
         ↓ (if None)
2. config.provider_name   + config.model (creates provider from name)
         ↓ (if None)
3. EDGEQUAKE_LLM_PROVIDER + EDGEQUAKE_MODEL (from env)
         ↓ (if not set)
4. ProviderFactory::from_env()  (auto-detect from API keys)
```

## Recommended Settings by Use Case

| Use Case | DPI | Model | Concurrency | Fidelity | Maintain Format |
|----------|-----|-------|-------------|----------|----------------|
| Quick text extraction | 100 | gpt-4.1-nano | 20 | tier1 | no |
| General documents | 150 | gpt-4.1-nano | 10 | tier2 | no |
| Academic papers | 200 | gpt-4.1 | 5 | tier3 | no |
| Books (format consistency) | 150 | gpt-4.1-mini | 1 | tier2 | yes |
| Forms & tables | 200 | claude-sonnet-4-20250514 | 5 | tier3 | no |
| Budget batch processing | 100 | gpt-4.1-nano | 20 | tier1 | no |
