# Examples

Real-world usage examples for common scenarios.

## Basic Usage

### Convert a PDF to Markdown

```bash
pdf2md document.pdf
```

Output goes to stdout. Redirect or use `-o`:

```bash
pdf2md document.pdf -o output.md
```

### Convert from a URL

```bash
pdf2md https://arxiv.org/pdf/1706.03762 -o attention_paper.md
```

### Inspect PDF Metadata (No API Key Needed)

```bash
pdf2md --inspect-only document.pdf
```

Output:
```
File:         document.pdf
Title:        Attention Is All You Need
Author:       Vaswani et al.
Pages:        15
PDF Version:  1.5
Encrypted:    false
```

## Page Selection

### Single Page

```bash
pdf2md --pages 1 document.pdf
```

### Page Range

```bash
pdf2md --pages 3-15 document.pdf -o chapters.md
```

### Specific Pages

```bash
pdf2md --pages 1,5,10,15 document.pdf -o selected.md
```

## Quality Control

### High-Fidelity (Academic Papers with Math)

```bash
pdf2md \
  --fidelity tier3 \
  --model gpt-4.1 \
  --dpi 200 \
  --max-tokens 8192 \
  paper.pdf -o paper.md
```

### Fast Extraction (Text Only)

```bash
pdf2md \
  --fidelity tier1 \
  --model gpt-4.1-nano \
  --dpi 100 \
  --concurrency 20 \
  document.pdf -o text.md
```

### Book with Consistent Formatting

```bash
pdf2md \
  --maintain-format \
  --separator hr \
  --metadata \
  book.pdf -o book.md
```

## Output Formats

### Markdown with Page Separators

```bash
pdf2md --separator hr document.pdf -o output.md
```

Produces:
```markdown
# Page 1 content...

---

# Page 2 content...

---

# Page 3 content...
```

### Markdown with Page Comments

```bash
pdf2md --separator comment document.pdf -o output.md
```

Produces:
```markdown
# Page 1 content...

<!-- page 2 -->

# Page 2 content...
```

### JSON Output

```bash
pdf2md --json --metadata document.pdf > output.json
```

```json
{
  "markdown": "# Full Document...",
  "pages": [
    {
      "page_num": 1,
      "markdown": "# Page 1...",
      "input_tokens": 1523,
      "output_tokens": 812,
      "duration_ms": 2341,
      "error": null
    }
  ],
  "metadata": {
    "title": "Document Title",
    "author": "Author",
    "page_count": 10
  },
  "stats": {
    "total_pages": 10,
    "processed_pages": 10,
    "failed_pages": 0,
    "total_input_tokens": 15230,
    "total_output_tokens": 8120,
    "total_duration_ms": 12500
  }
}
```

### With YAML Front-Matter

```bash
pdf2md --metadata document.pdf -o output.md
```

Produces:
```markdown
---
title: "Attention Is All You Need"
author: "Vaswani et al."
pages: 15
pdf_version: "1.5"
---

# Attention Is All You Need
## Abstract
...
```

## Provider-Specific Examples

### OpenAI (Default)

```bash
export OPENAI_API_KEY="sk-..."

# Cheapest
pdf2md --model gpt-4.1-nano document.pdf

# Best quality
pdf2md --model gpt-4.1 document.pdf
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

### Ollama (Free, Local)

```bash
ollama pull llava
pdf2md --provider ollama --model llava document.pdf
```

## Batch Processing

### Convert All PDFs in a Directory

```bash
for f in *.pdf; do
  echo "Converting: $f"
  pdf2md "$f" -o "${f%.pdf}.md"
done
```

### Parallel Batch with GNU Parallel

```bash
find . -name '*.pdf' | parallel -j4 pdf2md {} -o {.}.md
```

### Extract Text Only (Budget Mode)

```bash
for f in *.pdf; do
  pdf2md --fidelity tier1 --model gpt-4.1-nano --dpi 100 "$f" -o "${f%.pdf}.md"
done
```

## Encrypted PDFs

```bash
pdf2md --password "secret123" encrypted.pdf -o decrypted.md
```

## Custom System Prompt

Create a file with your prompt:

```bash
cat > my_prompt.txt << 'EOF'
You are a legal document specialist. Convert this page to Markdown.
Focus on: clause numbers, definitions, cross-references.
Preserve exact legal language — do not paraphrase.
EOF

pdf2md --system-prompt my_prompt.txt contract.pdf -o contract.md
```

## Library Usage (Rust)

### Basic Conversion

```rust
use edgequake_pdf2md::{convert, ConversionConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ConversionConfig::default();
    let output = convert("document.pdf", &config).await?;
    
    println!("{}", output.markdown);
    println!("Pages: {}/{}", output.stats.processed_pages, output.stats.total_pages);
    println!("Tokens: {} in, {} out", output.stats.total_input_tokens, output.stats.total_output_tokens);
    
    Ok(())
}
```

### Streaming API

```rust
use edgequake_pdf2md::{convert_stream, ConversionConfig};
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ConversionConfig::builder()
        .concurrency(5)
        .build()?;
    
    let mut stream = convert_stream("document.pdf", &config).await?;
    
    while let Some(result) = stream.next().await {
        match result {
            Ok(page) => println!("Page {} done ({} tokens)", page.page_num, page.output_tokens),
            Err(e) => eprintln!("Page error: {}", e),
        }
    }
    
    Ok(())
}
```

### Write to File

```rust
use edgequake_pdf2md::{convert_to_file, ConversionConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ConversionConfig::builder()
        .model("gpt-4.1-nano")
        .provider_name("openai")
        .build()?;
    
    let stats = convert_to_file("input.pdf", "output.md", &config).await?;
    
    println!("Converted {} pages in {}ms", stats.processed_pages, stats.total_duration_ms);
    
    Ok(())
}
```

### Inspect Without Converting

```rust
use edgequake_pdf2md::inspect;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let meta = inspect("document.pdf").await?;
    
    println!("Title: {:?}", meta.title);
    println!("Pages: {}", meta.page_count);
    println!("Version: {}", meta.pdf_version);
    
    Ok(())
}
```
