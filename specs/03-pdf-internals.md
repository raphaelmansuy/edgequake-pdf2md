# 03 — PDF Internals & Edge Cases

> **See also**: [Index](./00-index.md) · [Algorithm](./02-algorithm.md) · [Error Handling](./08-error-handling.md)
>
> **External**: [PDF 2.0 Specification (ISO 32000-2)](https://pdfa.org/resource/pdf-specification-index/) · [PDF/A Wikipedia](https://en.wikipedia.org/wiki/PDF/A) · [pdfium-render docs](https://docs.rs/pdfium-render)

---

## 1. PDF File Structure

A PDF is a hierarchical object database stored in a binary file. Understanding the structure reveals why text extraction is hard and why rasterisation is reliable.

```
PDF File Layout
──────────────────────────────────
%PDF-1.7              ← header (magic bytes)
%????                 ← comment with high bytes (binary indicator)

1 0 obj               ← indirect object (num gen obj)
  << /Type /Catalog   ← dictionary
     /Pages 2 0 R     ← reference to page tree root
  >>
endobj

2 0 obj
  << /Type /Pages
     /Kids [3 0 R 4 0 R 5 0 R]
     /Count 3
  >>
endobj

3 0 obj               ← Page object
  << /Type /Page
     /Parent 2 0 R
     /MediaBox [0 0 595.28 841.89]   ← A4 in points
     /Contents 6 0 R                 ← content stream
     /Resources <<
         /Font << /F1 7 0 R >>
         /XObject << /Im1 8 0 R >>
     >>
  >>
endobj

...

xref                  ← cross-reference table (byte offsets)
0 9
0000000000 65535 f
0000000015 00000 n
...

trailer
  << /Size 9
     /Root 1 0 R
  >>
startxref
3512              ← byte offset of xref
%%EOF             ← end of file
```

### 1.1 Content Stream

Each page has a content stream: a series of operator-operand pairs that describe what to draw:

```
BT                    % Begin text
/F1 12 Tf             % Set font F1, size 12
100 700 Td            % Move to position (100, 700)
(Hello, World!) Tj    % Show text string
ET                    % End text

q                     % Save graphics state
200 300 100 150 re    % Rectangle path
f                     % Fill
Q                     % Restore graphics state

/Im1 Do               % Draw XObject image Im1
```

**Why coordinate-only text fails**: Text is placed by absolute `Td`/`Tm` coordinates. There is no inherent reading order, paragraph structure, or semantic markup. Multi-column text, footnotes, and side-bars produce interleaved coordinate sequences.

### 1.2 Font Encoding Complexity

```
PDF Font Types
├── Type 1 (PostScript)
│    └── Encoding: StandardEncoding, MacRomanEncoding, WinAnsiEncoding, or custom
├── TrueType / OpenType
│    └── Encoding: MacRomanEncoding, WinAnsiEncoding, or custom ToUnicode CMap
├── Type 0 (Composite / CID fonts)
│    └── CIDToGIDMap + ToUnicode CMap required for text extraction
│    └── Unicode may be absent → text extraction impossible
├── Type 3 (user-defined glyph programs)
│    └── Usually no ToUnicode; purely visual
└── MMType1 (Multiple Master)
```

**Impact**: Without a `ToUnicode` CMap, text extraction returns garbage or empty strings. Rasterisation is unaffected since Pdfium renders the glyphs themselves. This is the #1 reason why rendering → VLM is superior to text extraction.

---

## 2. Edge Cases Catalogue

### 2.1 Password-Protected PDFs

```
PDF encryption levels:
  40-bit RC4  (PDF 1.1–1.3)   ← trivially broken
  128-bit RC4 (PDF 1.4)       ← weak
  128-bit AES (PDF 1.6)       ← acceptable
  256-bit AES (PDF 2.0)       ← current standard

Two password types:
  User password: required to OPEN the document
  Owner password: required to PRINT/COPY/MODIFY

Pdfium behavior:
  - User password required to render → pass as &str to load_pdf_from_file
  - Owner restrictions (no-copy) do NOT prevent rendering
  - Encrypted-no-password PDFs (restrictions only) → open without password ✓
```

**Implementation**: Accept `Option<&str>` password parameter in `ConversionConfig`. The key constraint: **owner password restrictions do not block rendering**. We only need the user password.

### 2.2 Scanned PDFs (Image-Only)

```
A scanned PDF has no text layer:
  Page content stream: /Im1 Do  (just draw one full-page image)
  
  Text extraction result: ""  (empty)
  Rasterisation result:  correct visual rendering ✓
  
Detection: pdfium page.text().all_text() == "" → scanned page heuristic
  (Note: some pages legitimately have no text, e.g. purely decorative figures)
```

**Impact**: The VLM-based approach handles scanned PDFs automatically — the LLM performs OCR on the rasterised image. Traditional text extraction completely fails. This is the core strength of the approach.

### 2.3 Mixed-Media Pages

A page may contain:
- Text rendered by the PDF engine (vector text)
- Raster images (JPEG, PNG, JBIG2 compressed)
- Vector graphics (paths, fills, strokes)
- Form XObjects (reusable content)
- Annotations (comments, links, form fields)
- Transparency groups

All of these are correctly rendered by Pdfium since it implements the full PDF rendering model. The VLM sees the final composed page, identical to what Acrobat shows.

### 2.4 Page Rotation

```
PDF MediaBox: [0 0 595 842]  — always lower-left origin
PDF Rotate:   0 | 90 | 180 | 270  — clockwise rotation

A landscape A4 scanned and saved as portrait+rotate=90:
  MediaBox: [0 0 595 842]  (portrait dimensions)
  Rotate: 90               (rotate 90° clockwise → displays as landscape)

pdfium-render PdfRenderConfig:
  .rotate_if_landscape(PdfPageRenderRotation::Degrees90, true)

This handles the common case, but complex rotation logic requires
per-page inspection:
  page.page_size()  → PdfPoints width/height
  if width > height → landscape page
```

### 2.5 Non-Standard Page Sizes

```
Standard A4:  595.28 × 841.89 points  (210mm × 297mm)
US Letter:    612 × 792 points        (8.5" × 11")
Legal:        612 × 1008 points       (8.5" × 14")
A3:           841.89 × 1190.55 points (297mm × 420mm)
Custom:       variable (scientific posters, roll plots)

Very large pages (e.g. A0 = 2383 × 3370 points):
  At 150 DPI: 2383/72*150 = 4965px wide
  PNG size: ~2MB per page
  Mitigation: cap max_pixels = 2000 × 2000
    scale both dimensions proportionally to fit cap
```

**Implementation**: `max_rendered_pixels` config option (default: 4_000_000 = ~2000×2000).

### 2.6 PDF/A, PDF/E, PDF/X Variants

| Variant | Purpose | Impact on us |
|---------|---------|--------------|
| PDF/A-1b | Long-term archiving | No JavaScript; all fonts embedded → renames fine |
| PDF/A-2 | With transparency & JPEG2000 | Fine |
| PDF/X | Print exchange | Colour profiles; renders correctly |
| PDF/E | Engineering | Large vector drawings; may hit pixel cap |
| PDF/UA | Universal accessibility | Actual tag structure available; ignore for v1 |

### 2.7 Linearised PDFs

Linearised PDFs are optimised for byte-range HTTP delivery (web-optimised). The file starts with the first page's data. `pdfium-render` handles these transparently. Not a special case for us.

### 2.8 Incremental Updates

PDF supports appending to a file (incremental update). This creates multiple `%%EOF` markers. Pdfium handles this transparently by processing the latest xref table.

### 2.9 Cross-Reference Streams (PDF 1.5+)

Modern PDFs may use cross-reference streams (compressed object streams) instead of the traditional `xref` table. Example:

```
15 0 obj
  << /Type /XRef
     /W [1 3 1]     ← widths of fields
     /Filter /FlateDecode
     /Length 42
  >>
stream
...compressed xref data...
endstream
```

Pdfium handles this transparently. Not a special case.

### 2.10 Corrupt and Partial PDFs

```
Types of corruption:
  Truncated file: %%EOF missing or incomplete objects
  Missing xref: pdfium falls back to linear object scan
  Object loop: circular reference in /Kids tree
  Invalid stream length: actual data != declared /Length

Pdfium behavior:
  - Best-effort recovery for common truncation
  - Returns PdfiumError for unrecoverable corruption

Our behavior:
  - Attempt to open; if error → Pdf2MdError::CorruptPdf { path, details }
  - Partial recovery not attempted (v1)
```

### 2.11 Very Long Document Processing

```
Large document considerations:
  - 500+ page documents common in contracts, books, reports
  - pdfium holds the entire document handle open
  - We stream page-by-page, so RAM ≈ constant per rendered page
  
Memory model:
  pdfium doc handle: ~2–10MB for index structures
  Bitmap buffer per page: width_px × height_px × 4 bytes
    e.g. 1240 × 1754 × 4 = 8.7MB per page
  PNG encoded per page: ~300KB
  base64 per page: ~400KB
  
  At concurrency=10: peak ≈ 10 × 400KB = 4MB of base64 strings
  Plus pdfium doc + 1 rendered bitmap at a time = ~12MB total
  
  This is excellent. Even 1000-page docs are fine.
```

### 2.12 PDF Forms (Interactive Form Fields)

PDF forms contain `AcroForm` interactive widgets (text boxes, checkboxes, radio buttons, dropdowns). When flattened (values filled), they appear as text in the rendering. When not flattened, the field widgets are rendered by Pdfium using their `appearance streams` (AP entries) or default rendering. In both cases, the visual content is preserved in the rasterisation.

For document input forms, `maintain_format=false` is recommended since each page is independent.

### 2.13 Annotations

```
PDF annotation types:
  /Text       ← sticky notes
  /FreeText   ← text boxes
  /Link       ← hyperlinks
  /Highlight  ← text highlights
  /Stamp      ← rubber stamps ("CONFIDENTIAL" etc.)
  /Popup      ← popup windows for /Text annotations
  
PdfRenderConfig rendering annotations:
  renderAnnotations: true (default) in pdfium
  
Visible annotations (stamps, free text, highlights) appear in rendered image.
VLM may extract annotation text as regular content.
Hidden annotations (e.g. /F 2 flag = hidden) are not rendered.
```

---

## 3. Token Budget

The LLM context window constrains how much content can be in each VLM call.

```
Input token estimation:
  ImageData tokens ≈ pixels/1000 (rough OpenAI estimate for high-detail)
    1240×1754 image at high detail ≈ 800–1200 input tokens
    
  System prompt ≈ 150 tokens
  User prompt   ≈ 50 tokens
  
  Total input per page: ~1000–1500 tokens

Output token estimation:
  Dense text page (academic paper): 400–800 tokens output markdown
  Light page (title page, blank): 10–50 tokens
  Table-heavy page: 300–600 tokens
  
  Average: ~500 tokens output per page

Cost estimate per page (OpenAI gpt-4o, Feb 2026 pricing ~$10/$30 per M):
  Input:  1200 tokens × $10/1M = $0.012
  Output:  500 tokens × $30/1M = $0.015
  Total per page: ~$0.027 (~$0.03)
  
  50-page document: ~$1.40
  10-page document: ~$0.28

Context window check:
  Model max context: 128K tokens (gpt-4o)
  Our usage per call: ~2000 tokens
  We never hit context window limits (each page = independent call)
```

---

## 4. PDF Feature Support Matrix

| Feature | Pdfium Support | Our Handling |
|---------|---------------|--------------|
| Text rendering (Type 1, TT, OTF) | ✓ Full | Automatic |
| CID/composite fonts | ✓ Full | Automatic |
| Images (JPEG, PNG, JBIG2, CCITTFax) | ✓ Full | Automatic |
| Transparency | ✓ Full | Automatic |
| Layers (OCG/Optional Content Groups) | ✓ (all visible) | All layers visible |
| 3D annotations (U3D, PRC) | ✗ Partial | Rendered as gray box |
| Video/audio annotations | ✗ None | Rendered as icon |
| JavaScript | ✗ Not executed | Static render only |
| PDF forms (flattened) | ✓ Full | Automatic |
| PDF forms (interactive, unflattened) | ✓ Rendered | Widgets show default/AP |
| Digital signatures (visual) | ✓ Visual only | Rendered |
| Watermarks | ✓ Full | Rendered |
| XFA forms | ✗ Limited | May render incorrectly |

**Note on XFA forms**: XFA (XML Forms Architecture) is a complex XML-based alternative to AcroForm used by some government and enterprise PDFs. Pdfium 6.0+ has partial XFA support. For XFA-heavy docs, recommend pre-converting with Adobe Reader. Flagged via `ConversionWarning::XfaContent`.

---

## 5. PDF Standards Reference

| Standard | Full Name | Relevance |
|----------|-----------|-----------|
| ISO 32000-1 | PDF 1.7 | Core specification; most PDFs |
| ISO 32000-2 | PDF 2.0 | Current; encryption, AES-256 |
| ISO 19005 | PDF/A | Archiving variants (PDF/A-1, -2, -3, -4) |
| ISO 15930 | PDF/X | Print exchange |
| ISO 24517 | PDF/E | Engineering documents |
| ISO 14289 | PDF/UA | Universal accessibility |

Key reference: [Adobe PDF Reference Archive](https://www.adobe.com/devnet/pdf.html)
