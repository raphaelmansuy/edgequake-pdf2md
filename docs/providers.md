# Supported Providers & Models

`edgequake-pdf2md` works with any Vision Language Model (VLM) supported by [edgequake-llm](https://crates.io/crates/edgequake-llm) (v0.2.7+).

## Provider Overview

| Provider | Env Variable | Vision Models | Notes |
|----------|-------------|---------------|-------|
| **OpenAI** | `OPENAI_API_KEY` | gpt-4.1-nano, gpt-4.1-mini, gpt-4.1, gpt-4o | Default provider, best cost/quality |
| **Anthropic** | `ANTHROPIC_API_KEY` | claude-sonnet-4-20250514, claude-haiku-4-20250514 | High accuracy for complex layouts |
| **Google Gemini** | `GEMINI_API_KEY` | gemini-2.0-flash, gemini-2.5-pro | Large context window |
| **Azure OpenAI** | `AZURE_OPENAI_API_KEY` | gpt-4o (deployed) | Enterprise compliance, native vision |
| **Ollama** | (local) | llava, llama3.2-vision | Free, runs locally |
| **OpenAI-compatible** | `OPENAI_API_KEY` + custom base URL | Any vision model | vLLM, LiteLLM, Together AI, etc. |

## Model Comparison

### OpenAI Models

| Model | Input $/1M | Output $/1M | Context | Speed | Quality | Best For |
|-------|-----------|-------------|---------|-------|---------|----------|
| **gpt-4.1-nano** | $0.10 | $0.40 | 1M | ★★★★★ | ★★★ | Default — fast & cheap |
| **gpt-4.1-mini** | $0.40 | $1.60 | 1M | ★★★★ | ★★★★ | Good balance |
| **gpt-4.1** | $2.00 | $8.00 | 1M | ★★★ | ★★★★★ | Best quality |
| **gpt-4o** | $2.50 | $10.00 | 128K | ★★★ | ★★★★ | Legacy |

### Anthropic Models

| Model | Input $/1M | Output $/1M | Context | Speed | Quality | Best For |
|-------|-----------|-------------|---------|-------|---------|----------|
| **claude-sonnet-4-20250514** | $3.00 | $15.00 | 200K | ★★★ | ★★★★★ | Complex tables, academic papers |
| **claude-haiku-4-20250514** | $0.80 | $4.00 | 200K | ★★★★ | ★★★★ | Good quality, moderate cost |

### Google Gemini Models

| Model | Input $/1M | Output $/1M | Context | Speed | Quality | Best For |
|-------|-----------|-------------|---------|-------|---------|----------|
| **gemini-2.0-flash** | $0.10 | $0.40 | 1M | ★★★★★ | ★★★ | Budget-friendly |
| **gemini-2.5-pro** | $1.25 | $10.00 | 1M | ★★★ | ★★★★★ | Highest accuracy |

### Local Models (Ollama)

| Model | Cost | Speed | Quality | Notes |
|-------|------|-------|---------|-------|
| **llava** | Free | ★★★ | ★★ | 7B params, basic vision |
| **llama3.2-vision** | Free | ★★★ | ★★★ | 11B/90B params, better accuracy |

## Cost Estimates

### Per-Page Cost

A typical page at 150 DPI generates ~1,500 input tokens and ~800 output tokens.

| Model | Per Page | 10 Pages | 50 Pages | 100 Pages |
|-------|----------|----------|----------|-----------|
| gpt-4.1-nano | $0.0005 | $0.005 | $0.02 | $0.05 |
| gpt-4.1-mini | $0.002 | $0.02 | $0.09 | $0.19 |
| gpt-4.1 | $0.009 | $0.09 | $0.47 | $0.94 |
| claude-sonnet-4-20250514 | $0.017 | $0.17 | $0.83 | $1.65 |
| gemini-2.0-flash | $0.0005 | $0.005 | $0.02 | $0.05 |

> **Note:** Actual costs vary with page complexity. Dense pages with tables and math produce more output tokens.

## Usage Examples

### OpenAI (default)

```bash
export OPENAI_API_KEY="sk-..."

# Use default model (gpt-4.1-nano)
pdf2md document.pdf

# Use a specific model
pdf2md --model gpt-4.1 document.pdf

# Use gpt-4.1-mini for a good balance
pdf2md --model gpt-4.1-mini document.pdf
```

### Anthropic

```bash
export ANTHROPIC_API_KEY="sk-ant-..."
pdf2md --provider anthropic --model claude-sonnet-4-20250514 document.pdf
```

### Google Gemini

```bash
export GEMINI_API_KEY="AI..."
pdf2md --provider gemini --model gemini-2.0-flash document.pdf
```

### Azure OpenAI

As of `edgequake-llm` v0.2.7, the Azure OpenAI provider is a full native rewrite using the
`async-openai` `AzureConfig` trait — no hand-rolled HTTP, full vision support.

#### Constructor options

| Constructor | When to use |
|------------|-------------|
| `from_env()` | Standard `AZURE_OPENAI_*` env vars |
| `from_env_contentgen()` | Enterprise `AZURE_OPENAI_CONTENTGEN_*` naming scheme |
| `from_env_auto()` | Tries CONTENTGEN first, falls back to standard automatically |

#### Standard env vars

```bash
export AZURE_OPENAI_API_KEY="..."
export AZURE_OPENAI_ENDPOINT="https://<resource>.openai.azure.com"
export AZURE_OPENAI_DEPLOYMENT_NAME="gpt-4o"        # chat deployment
export AZURE_OPENAI_API_VERSION="2024-02-01"
```

#### Enterprise (ContentGen) env vars

```bash
export AZURE_OPENAI_CONTENTGEN_API_KEY="..."
export AZURE_OPENAI_CONTENTGEN_API_ENDPOINT="https://<resource>.openai.azure.com"
export AZURE_OPENAI_CONTENTGEN_MODEL_DEPLOYMENT="gpt-4o"
export AZURE_OPENAI_CONTENTGEN_API_VERSION="2024-02-01"
```

#### CLI usage

```bash
# Standard AZURE_OPENAI_* vars
pdf2md --provider azure --model gpt-4o document.pdf

# URL images are passed to Azure API directly (no base64 re-encoding)
pdf2md --provider azure --model gpt-4o https://example.com/document.pdf
```

> **Note:** `ProviderFactory::from_env()` in v0.2.7 auto-detects Azure when
> `AZURE_OPENAI_CONTENTGEN_API_KEY` or `AZURE_OPENAI_API_KEY` plus endpoint
> are set — no `--provider azure` flag needed.

### Ollama (Local)

```bash
ollama pull llava
pdf2md --provider ollama --model llava document.pdf
```

### edgequake-litellm (Python)

`edgequake-litellm` v0.1.3 adds Azure routing support:

```python
import edgequake_litellm as litellm

# Route to Azure OpenAI
response = litellm.completion(
    model="azure/gpt-4o",          # azure/<deployment-name>
    messages=[{"role": "user", "content": "Summarise this PDF page."}],
)
print(response.choices[0].message.content)

# List all supported providers (includes "azure" in v0.1.3)
print(litellm.list_providers())
```

### Auto-Detection

If you don't specify `--provider`, the tool auto-detects from environment variables:
1. Checks `EDGEQUAKE_LLM_PROVIDER` + `EDGEQUAKE_MODEL`
2. Falls back to `ProviderFactory::from_env()` which checks for API keys in order

## Choosing a Model

```
Need cheapest option?
  └── gpt-4.1-nano or gemini-2.0-flash ($0.02 per 50 pages)

Need best quality?
  └── gpt-4.1 or gemini-2.5-pro

Need complex table extraction?
  └── claude-sonnet-4-20250514

Need offline/private processing?
  └── Ollama + llama3.2-vision

Need enterprise compliance?
  └── Azure OpenAI deployment
```
