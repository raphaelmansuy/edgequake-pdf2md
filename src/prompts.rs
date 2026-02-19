//! System prompts for VLM-based PDF-to-Markdown conversion.
//!
//! Centralising every prompt here serves two purposes:
//!
//! 1. **Single source of truth** — changing the default behaviour (e.g. adding
//!    a new rule or tweaking table handling) requires editing exactly one place.
//!
//! 2. **Testability** — unit tests can import and inspect prompts directly
//!    without spinning up a real VLM, making prompt regressions easy to catch.
//!
//! Callers can override the default via [`crate::config::ConversionConfig::system_prompt`];
//! the constants here are used only when no override is provided.

/// Default system prompt for converting a PDF page image to Markdown.
///
/// This prompt is used when `ConversionConfig::system_prompt` is `None`.
pub const DEFAULT_SYSTEM_PROMPT: &str = r#"You are an expert document converter. Your task is to convert a PDF page image to clean, well-structured Markdown.

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

5. FORMULAS
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
   - Start directly with the page content"#;

/// Additional instruction appended when `maintain_format` is enabled.
///
/// The placeholder `{prior_page}` must be replaced with the previous page's
/// markdown content before use.
pub const MAINTAIN_FORMAT_SUFFIX: &str = r#"

8. FORMAT CONTINUITY
   The previous page's content is provided as context. Ensure your output
   is stylistically consistent with the previous page. Continue any
   numbered lists, subsections, or running text that began on the previous page."#;

/// Build the context message for maintain_format mode.
///
/// This is sent as a separate system message containing the prior page's content.
pub fn maintain_format_context(prior_page: &str) -> String {
    format!(
        "Markdown must maintain consistent formatting with the following page:\n\n\"\"\"{}\"\"\"",
        prior_page
    )
}
