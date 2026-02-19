# 04 — Markdown Output Specification

> **See also**: [Index](./00-index.md) · [Algorithm](./02-algorithm.md) · [Testing](./09-testing-strategy.md)
>
> **External**: [CommonMark Specification](https://spec.commonmark.org/) · [GitHub Flavored Markdown](https://github.github.com/gfm/) · [Pandoc Markdown Extensions](https://pandoc.org/MANUAL.html#pandocs-markdown)

---

## 1. Target Markdown Dialect

The output targets **CommonMark 0.31.2** with the following **GFM (GitHub Flavored Markdown)** extensions enabled:

| Extension | Reason |
|-----------|--------|
| GFM tables | Most PDFs contain tables |
| GFM task lists | Checkbox-like forms |
| GFM strikethrough | Editorial documents may use strikethrough |
| Fenced code blocks | Technical documents contain code |

**Explicitly excluded extensions**:
- `$...$` LaTeX math (not universally supported; use verbatim or fenced block)
- Footnotes (handled via GFM-style endnotes if supported by renderer)
- Raw HTML blocks (never emit; our output should be HTML-safe)

Normative reference: [CommonMark spec](https://spec.commonmark.org/0.31.2/)

---

## 2. Fidelity Tiers

We define three quality tiers to set realistic expectations:

```
Tier 1 — Basic (minimum viable)
  ✓ Body text preserved correctly
  ✓ Paragraphs separated by blank lines
  ✓ Headings (h1–h4) detected
  ✓ Bold / italic preserved
  ✓ Ordered and unordered lists
  ✓ Code blocks (monospaced sections)
  ✗ Tables may be linearised to text
  ✗ Formulas may be verbatim text
  ✗ Figure captions may be lost
  
Tier 2 — Structural (recommended)
  ✓ Everything in Tier 1
  ✓ Tables in GFM pipe format
  ✓ Figure captions preserved as italics below image-references
  ✓ Footnotes as inline [^n] references
  ✓ Section numbering preserved
  ✗ Mathematical formulas as approximate text
  ✗ Complex tables (merged cells) may use simplified format
  
Tier 3 — High-fidelity (expensive, gpt-4o/claude-3.5-sonnet recommended)
  ✓ Everything in Tier 2
  ✓ LaTeX math formulas in $...$ and $$...$$ blocks
  ✓ Merged-cell tables use HTML <table> fallback
  ✓ Captions and cross-references preserved
  ✓ Multi-column layout correctly ordered
```

**Default**: Tier 2. Selectable via `ConversionConfig::fidelity_tier`.

---

## 3. Element Mapping: PDF Visual → Markdown

### 3.1 Text

| PDF Visual Element | Markdown Output | Notes |
|---|---|---|
| Large centered text (title) | `# Title Text` | Heuristic: font≥18pt, centered |
| Section heading (bold, large) | `## Section` | Font size heuristic |
| Subsection | `### Subsection` | |
| Body paragraph | Plain paragraph | Blank line separation |
| **Bold** text | `**bold**` | |
| *Italic* text | `*italic*` | |
| ***Bold italic*** | `***bold italic***` | |
| `Monospaced` inline | `` `mono` `` | CJK fonts excepted |
| Strikethrough text | `~~text~~` | GFM extension |
| ALL CAPS heading | `## HEADING` (normalised) | Preserve casing |
| Small caps | Normal text (cannot distinguish) | |
| Superscript (footnote ref) | `[^1]` when followed by footer | |
| Subscript | `_{sub}` or plain text | Tier 3 only |
| Hyperlink (annotated) | `[text](url)` | From /Link annotation |
| Underlined (visual) | Plain text | Underline = no Markdown equivalent |

### 3.2 Headings

The VLM infers heading level from visual cues. Our system prompt instructs:

```
Heading level heuristics (instruct the VLM):
  - The largest, most prominent text on the page → # (h1)
  - Section numbers (1. Introduction) at top level → ##
  - Subsection numbers (1.1 Background) → ###
  - Minor headings → ####
  - Never use h5/h6 (rare in academic/business docs)
  - If ambiguous, prefer a lower heading level (flatter hierarchy)
```

### 3.3 Lists

```
PDF rendering            → Markdown output

• Item 1                 → - Item 1
• Item 2                 → - Item 2
  ◦ Nested item          →   - Nested item

1. First item            → 1. First item
2. Second item           → 2. Second item
   a. Sub-item           →    a. Sub-item

☐ Task item              → - [ ] Task item
☑ Checked item           → - [x] Checked item
```

**Challenge**: Detecting list indentation levels from rendered bitmaps relies entirely on the VLM's spatial reasoning. GPT-4o and Claude 3.5 Sonnet handle this well.

### 3.4 Tables

GFM pipe table format:

```markdown
| Column A | Column B | Column C |
|----------|----------|----------|
| a1       | b1       | c1       |
| a2       | b2       | c2       |
```

Rules:
- **Column alignment**: Use `:---` (left), `:---:` (center), `---:` (right) based on visual alignment
- **Header row**: Always present; if table has no header, duplicate first data row as header
- **Empty cells**: Use space `|   |` not empty pipes
- **Merged cells (rowspan/colspan)**: 
  - Tier 2: linearise → repeat value in each spanned cell
  - Tier 3: emit raw HTML `<table>` block

Example merged cell fallback (Tier 3):
```html
<table>
<tr><th colspan="2">Merged Header</th></tr>
<tr><td>A</td><td>B</td></tr>
</table>
```

### 3.5 Code and Technical Content

```
PDF rendering            → Markdown output

Monospaced block         → ``` block (fenced, no language tag)

                         → ```python
                         → def hello():
Python source code       →     print("hello")
                         → ```
(if language detectable) 

Command/terminal example → ```shell-session
$ ls -la                 → $ ls -la
                         → ```

Inline code in prose     → `variable_name`
```

**Language detection**: Instruct VLM to tag fenced blocks with language name when visually obvious (syntax highlighting, context). Acceptable languages: `python`, `rust`, `javascript`, `typescript`, `java`, `c`, `cpp`, `sql`, `bash`, `sh`, `json`, `yaml`, `toml`, `xml`, `html`, `css`.

### 3.6 Mathematical Formulas

```
Fidelity Tier 2:
  Simple formula like E = mc²  → "E = mc²" (text approximation)
  Complex integral             → [formula - see original document]

Fidelity Tier 3 (requires gpt-4o or claude-3.5-sonnet):
  Inline formula               → $E = mc^2$
  Display formula              → $$\int_{0}^{\infty} f(x) dx$$
  Matrix                       → $\begin{pmatrix} a & b \\ c & d \end{pmatrix}$
```

**Warning**: LaTeX math output is only reliable with models fine-tuned on mathematical content (GPT-4o, Claude 3.5 Sonnet). Local models via Ollama may produce incorrect LaTeX.

### 3.7 Images and Figures

The VLM rendering approach cannot embed the original image pixels into Markdown (the output is text). Instead:

```
Figure handling options:

1. Caption-only (default):
   > **Figure 1**: Description of the figure as interpreted by VLM

2. Placeholder reference:
   ![Figure 1: Description extracted by VLM](./figures/page3_fig1.png)
   (Only if --extract-images flag is set; images extracted as PNG files)

3. Table substitute (charts/graphs):
   If VLM can read data from a bar chart or line graph,
   it may represent it as a Markdown table.
```

**Config**: `ConversionConfig::extract_images: bool` (default false). Image extraction writes `{output_stem}/figures/page{N}_fig{M}.png` files.

### 3.8 Headers and Footers

Running headers and footers typically appear at fixed positions on every page. The VLM is instructed to:

```
System prompt instruction:
  "Ignore repeated page headers and footers (page numbers, document 
   titles repeated on each page). Do not include them in your output
   unless they contain unique content specific to that page."
```

Edge case: First page headers may contain unique document metadata (title, author, date) and should be preserved.

### 3.9 Page Breaks

Page markers are optional (configured via `ConversionConfig::page_separator`):

```rust
pub enum PageSeparator {
    None,                           // pages joined with "\n\n"
    HorizontalRule,                 // "---\n\n" between pages
    Comment(String),                // "<!-- page N -->\n\n"
    Custom(String),                 // user-specified string
}
```

Default: `PageSeparator::None` (seamless document assembly).

---

## 4. System Prompt

The system prompt is the most critical configuration for output quality. Full prompt:

```
You are an expert document converter. Your task is to convert a PDF page
image to clean, well-structured Markdown.

Follow these rules precisely:

1. TEXT PRESERVATION
   - Preserve ALL text content completely and accurately
   - Maintain the reading order as a human would read the page
   - Correct obvious OCR-like errors only if you are completely certain

2. STRUCTURE
   - Use # for the main page title (at most one per page)
   - Use ## for major sections, ### for subsections, #### for minor headings
   - Use - for unordered lists and 1. 2. 3. for ordered lists
   - Preserve list nesting with indentation
   - Use **bold** and *italic* to match the visual emphasis

3. TABLES
   - Convert tables to GFM pipe format
   - Add alignment markers (:---, :---:, ---:) matching visual alignment
   - If a table is too complex for pipe format, use HTML table markup

4. CODE
   - Wrap code blocks in triple backticks with language identifier
   - Wrap inline code in single backticks

5. FORMULAS (if requested at Tier 3)
   - Render mathematical expressions using LaTeX: $inline$ and $$display$$

6. WHAT TO IGNORE
   - Page numbers (bottom/top of page)
   - Repeated headers/footers that appear on every page
   - Decorative borders and lines that carry no content meaning

7. OUTPUT FORMAT
   - Output ONLY the Markdown content
   - Do NOT wrap in ```markdown fences
   - Do NOT add commentary or explanations
   - Do NOT add "Page X of Y" markers
   - Start directly with the page content
```

**Variation for maintain_format mode** (appended to base prompt):
```
8. FORMAT CONTINUITY
   The previous page's content is provided as context. Ensure your output
   is stylistically consistent with the previous page. Continue any 
   numbered lists, subsections, or running text that began on the previous page.
```

---

## 5. Output Document Structure

```markdown
---
title: "Document Title"
source: "path/to/input.pdf"
pages: 42
generated: "2026-02-19T14:30:00Z"
provider: "openai/gpt-4o"
---

<!-- Content begins here, assembled from per-page outputs -->

# Chapter 1: Introduction

Body text paragraph...

## 1.1 Background

...

<!-- Page separator (if configured) -->

---

# Chapter 2: Methods

...
```

The YAML front-matter block is optional (`ConversionConfig::include_metadata: bool`, default `false`).

---

## 6. Post-Processing Rules

Performed by the Rust post-processor (not the VLM):

```
1. Strip outer markdown fences:
   Pattern: ^```(?:markdown)?\n([\s\S]*)\n```$
   → Remove fences, keep inner content

2. Normalise line endings:
   \r\n → \n
   \r   → \n

3. Trim trailing whitespace per line:
   "  text   " → "  text"

4. Collapse excessive blank lines:
   \n{4,} → \n\n\n  (max 2 blank lines between paragraphs)

5. Ensure file ends with single newline:
   content.trim_end() + "\n"

6. Normalise heading spacing:
   Ensure blank line before and after each heading:
   regex: (?<!^)(#{1,6} ) → \n\n\1

7. Fix broken GFM tables:
   If table has no separator row → insert | --- | header separator

8. Remove zero-width spaces and other invisible Unicode:
   U+200B, U+FEFF, U+00AD (soft hyphen) → removed
```

---

## 7. Quality Metrics

To measure output quality against reference Markdown (for CI golden tests):

| Metric | Method | Tool |
|--------|--------|------|
| **BLEU** | n-gram overlap vs. reference | sacrebleu |
| **ROUGE-L** | Longest common subsequence | rouge-rs |
| **Structure match** | Heading/table/list count diff | Custom |
| **CER** (Character Error Rate) | Edit distance / len | Custom |
| **GFM validity** | parseable without errors | pulldown-cmark |

Target thresholds (Tier 2, gpt-4o):
- CER ≤ 0.05 on text content
- Structure match ≥ 0.90 (headings, tables, lists)
- GFM parse: 0 errors

See [09-testing-strategy.md](./09-testing-strategy.md) for golden corpus details.
