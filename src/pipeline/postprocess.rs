//! Post-processing: deterministic cleanup of VLM-generated Markdown.
//!
//! ## Why is post-processing necessary?
//!
//! Even well-prompted VLMs occasionally introduce artefacts that are
//! *semantically correct* from the model's perspective but *structurally
//! invalid* \u2014 for example:
//!
//! - Wrapping output in ` ```markdown ... ``` ` fences despite the prompt
//!   saying "do not wrap in fences"
//! - Inserting a reference to `![figure](image.png)` when no file exists
//! - Using Windows-style `\r\n` line endings
//! - Hallucinating spurious separator rows inside tables
//!
//! This module applies 10 cheap, deterministic regex/string rules that fix
//! model quirks without touching content. Keeping them here rather than in the
//! prompt means the prompt stays focused on *what to extract*, not on
//! *formatting edge-cases*. Each rule is independently testable.
//!
//! ## Rule Order
//!
//! Rules must run in this specific order: normalise line endings before
//! trimming, strip fences before heading-spacing so heading detection works
//! on clean input, and remove image links before the final-newline pass.

use once_cell::sync::Lazy;
use regex::Regex;

/// Apply all post-processing rules to the raw VLM output.
///
/// Runs 10 deterministic cleanup passes in a defined order. Each pass is a
/// pure function (`&str → String`) with no shared state, making the pipeline
/// easy to extend or re-order without side effects.
///
/// Rules (applied in order):
/// 1. Strip outer markdown fences (models sometimes disobey the prompt)
/// 2. Normalise line endings (CRLF → LF)
/// 3. Trim trailing whitespace per line
/// 4. Collapse 3+ consecutive blank lines down to 2
/// 5. Ensure heading lines have a blank line before them
/// 6. Fix broken GFM tables missing a separator row
/// 7. Remove spurious mid-table separator rows inserted by the model
/// 8. Remove hallucinated image links (`![...]()` with fake/placeholder URLs)
/// 9. Strip invisible Unicode (zero-width spaces, BOM, soft hyphens, etc.)
/// 10. Ensure the file ends with exactly one newline
pub fn clean_markdown(input: &str) -> String {
    let s = strip_markdown_fences(input);
    let s = normalise_line_endings(&s);
    let s = trim_trailing_whitespace(&s);
    let s = collapse_blank_lines(&s);
    let s = normalise_heading_spacing(&s);
    let s = fix_broken_tables(&s);
    let s = remove_mid_table_separators(&s);
    let s = remove_hallucinated_images(&s);
    let s = remove_invisible_chars(&s);
    ensure_final_newline(&s)
}

// ── Rule 1: Strip outer markdown fences ──────────────────────────────────────

static RE_OUTER_FENCES: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?s)^```(?:markdown)?\n(.*)\n```\s*$").unwrap());

fn strip_markdown_fences(input: &str) -> String {
    if let Some(caps) = RE_OUTER_FENCES.captures(input.trim()) {
        caps[1].to_string()
    } else {
        input.to_string()
    }
}

// ── Rule 2: Normalise line endings ───────────────────────────────────────────

fn normalise_line_endings(input: &str) -> String {
    input.replace("\r\n", "\n").replace('\r', "\n")
}

// ── Rule 3: Trim trailing whitespace per line ────────────────────────────────

fn trim_trailing_whitespace(input: &str) -> String {
    input
        .lines()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

// ── Rule 4: Collapse excessive blank lines ───────────────────────────────────

static RE_BLANK_LINES: Lazy<Regex> = Lazy::new(|| Regex::new(r"\n{4,}").unwrap());

fn collapse_blank_lines(input: &str) -> String {
    RE_BLANK_LINES.replace_all(input, "\n\n\n").to_string()
}

// ── Rule 5: Ensure file ends with single newline ─────────────────────────────

fn ensure_final_newline(input: &str) -> String {
    let trimmed = input.trim_end();
    if trimmed.is_empty() {
        String::from("\n")
    } else {
        format!("{}\n", trimmed)
    }
}

// ── Rule 6: Normalise heading spacing ────────────────────────────────────────

fn normalise_heading_spacing(input: &str) -> String {
    // Ensure a blank line before each heading (unless at the very start)
    let mut result = String::with_capacity(input.len() + 64);
    for (i, line) in input.lines().enumerate() {
        let is_heading =
            line.starts_with('#') && line.chars().nth(line.find(' ').unwrap_or(0)).is_some();
        if is_heading && i > 0 {
            // Remove any single trailing newline and ensure double
            let trimmed = result.trim_end_matches('\n');
            result.truncate(trimmed.len());
            result.push_str("\n\n");
        }
        result.push_str(line);
        result.push('\n');
    }
    result
}

// ── Rule 7: Fix broken GFM tables ───────────────────────────────────────────

/// Detects table rows (lines starting with `|`) and ensures a separator row
/// exists after the first row if missing.
fn fix_broken_tables(input: &str) -> String {
    let lines: Vec<&str> = input.lines().collect();
    let mut result = Vec::with_capacity(lines.len() + 10);
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];

        // Check if this looks like a table header row (but not a separator row itself)
        if is_table_row(line) && !is_separator_row(line) {
            result.push(line.to_string());

            // Check if next line is a separator row
            let next = lines.get(i + 1).copied().unwrap_or("");
            if is_table_row(next) && !is_separator_row(next) {
                // Insert separator row
                let col_count = line.matches('|').count().saturating_sub(1).max(1);
                let sep: String = std::iter::once("|")
                    .chain(std::iter::repeat_n(" --- |", col_count))
                    .collect();
                result.push(sep);
            }
            i += 1;
            continue;
        }

        result.push(line.to_string());
        i += 1;
    }

    result.join("\n")
}

fn is_table_row(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.len() > 2
}

fn is_separator_row(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') {
        return false;
    }
    // A separator row contains only |, -, :, and whitespace
    trimmed
        .chars()
        .all(|c| c == '|' || c == '-' || c == ':' || c == ' ')
}

// ── Rule 8: Remove invisible Unicode characters ─────────────────────────────

fn remove_invisible_chars(input: &str) -> String {
    input.replace(
        [
            '\u{200B}', '\u{FEFF}', '\u{00AD}', '\u{200C}', '\u{200D}', '\u{2060}',
        ],
        "",
    )
}

// ── Rule 9: Remove hallucinated image links ───────────────────────────────────
//
// VLMs sometimes invent placeholder image URLs (e.g. `image-url`, `chart.png`,
// `https://example.com/image.png`) for figures they cannot represent in text.
// We detect these by checking whether the URL looks like a real absolute HTTP(S)
// link that came from the document vs. a fabricated placeholder.
//
// Strategy: keep `![alt](url)` only when the url starts with `http://` or
// `https://` AND the host is not a known placeholder domain. Otherwise convert
// to `*alt*` (italic caption) so the text is not lost.

static RE_IMAGE: Lazy<Regex> = Lazy::new(|| Regex::new(r"!\[([^\]]*)\]\(([^)]*)\)").unwrap());

/// List of URL patterns that indicate a fabricated/placeholder image link.
fn is_placeholder_url(url: &str) -> bool {
    let u = url.trim();
    if u.is_empty() {
        return true;
    }
    // Local-looking or obviously fake URLs
    if !u.starts_with("http://") && !u.starts_with("https://") {
        return true;
    }
    // Known placeholder/example domains
    let fake_domains = [
        "example.com",
        "placeholder.com",
        "via.placeholder.com",
        "dummyimage.com",
        "lorempixel.com",
        "picsum.photos",
        "placehold.it",
    ];
    fake_domains.iter().any(|d| u.contains(d))
}

fn remove_hallucinated_images(input: &str) -> String {
    RE_IMAGE
        .replace_all(input, |caps: &regex::Captures<'_>| {
            let alt = caps[1].trim();
            let url = &caps[2];
            if is_placeholder_url(url) {
                // Replace with just an italic caption (preserve the description text)
                if alt.is_empty() {
                    String::new()
                } else {
                    format!("*{}*", alt)
                }
            } else {
                // Keep real image links as-is
                caps[0].to_string()
            }
        })
        .to_string()
}

// ── Rule 10: Remove spurious mid-table separator rows ───────────────────────
//
// Some VLMs output extra `| --- | --- |` separator rows in the *body* of a
// table (not just after the header). GFM only allows a separator in position 2
// (after the header row). Extra separators confuse Markdown renderers.
//
// Algorithm: scan for table blocks. Within each table block, only keep the
// *first* separator row (which must follow the header at line 2). All other
// separator rows in the body are removed.

fn remove_mid_table_separators(input: &str) -> String {
    let lines: Vec<&str> = input.lines().collect();
    let mut result: Vec<&str> = Vec::with_capacity(lines.len());
    let mut in_table = false;
    let mut table_line_count = 0usize;

    for line in &lines {
        if is_table_row(line) {
            if !in_table {
                in_table = true;
                table_line_count = 0;
            }
            table_line_count += 1;

            // Only allow separator at position 2 (table_line_count == 2)
            if is_separator_row(line) && table_line_count != 2 {
                // Skip this spurious separator
                continue;
            }
            result.push(line);
        } else {
            in_table = false;
            table_line_count = 0;
            result.push(line);
        }
    }

    result.join("\n")
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_fences() {
        let input = "```markdown\n# Hello\nWorld\n```";
        assert_eq!(strip_markdown_fences(input), "# Hello\nWorld");
    }

    #[test]
    fn test_strip_fences_no_lang() {
        let input = "```\n# Hello\nWorld\n```";
        assert_eq!(strip_markdown_fences(input), "# Hello\nWorld");
    }

    #[test]
    fn test_no_fences_passthrough() {
        let input = "# Hello\nWorld";
        assert_eq!(strip_markdown_fences(input), "# Hello\nWorld");
    }

    #[test]
    fn test_normalise_line_endings() {
        assert_eq!(normalise_line_endings("a\r\nb\rc"), "a\nb\nc");
    }

    #[test]
    fn test_trim_trailing_whitespace() {
        assert_eq!(
            trim_trailing_whitespace("  hello   \nworld  "),
            "  hello\nworld"
        );
    }

    #[test]
    fn test_collapse_blank_lines() {
        let input = "a\n\n\n\n\n\nb";
        assert_eq!(collapse_blank_lines(input), "a\n\n\nb");
    }

    #[test]
    fn test_ensure_final_newline() {
        assert_eq!(ensure_final_newline("hello"), "hello\n");
        assert_eq!(ensure_final_newline("hello\n\n\n"), "hello\n");
        assert_eq!(ensure_final_newline(""), "\n");
    }

    #[test]
    fn test_heading_spacing() {
        let input = "some text\n# Heading\nmore text";
        let result = normalise_heading_spacing(input);
        assert!(result.contains("\n\n# Heading\n"));
    }

    #[test]
    fn test_fix_broken_table() {
        let input = "| A | B |\n| 1 | 2 |";
        let result = fix_broken_tables(input);
        // Should have a separator row inserted
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(is_separator_row(lines[1]));
    }

    #[test]
    fn test_table_with_separator_unchanged() {
        let input = "| A | B |\n| --- | --- |\n| 1 | 2 |";
        let result = fix_broken_tables(input);
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 3); // No extra separator added
    }

    #[test]
    fn test_remove_invisible() {
        let input = "hello\u{200B}world\u{FEFF}foo\u{00AD}bar";
        assert_eq!(remove_invisible_chars(input), "helloworldfoobar");
    }

    #[test]
    fn test_remove_hallucinated_image_placeholder_url() {
        let input = "Some text\n![Chart Title](chart.png)\nMore text";
        let result = remove_hallucinated_images(input);
        assert!(
            !result.contains("!["),
            "Should remove image with local path"
        );
        assert!(
            result.contains("*Chart Title*"),
            "Should keep alt text as italic"
        );
    }

    #[test]
    fn test_remove_hallucinated_image_fake_url() {
        let input = "![Diagram](image-url)";
        let result = remove_hallucinated_images(input);
        assert!(!result.contains("!["));
        assert!(result.contains("*Diagram*"));
    }

    #[test]
    fn test_keep_real_image_link() {
        let input = "![Figure](https://arxiv.org/figures/fig1.png)";
        let result = remove_hallucinated_images(input);
        assert!(result.contains("![Figure]"), "Should keep real image link");
    }

    #[test]
    fn test_remove_mid_table_separator() {
        let input = "| A | B |\n| --- | --- |\n| 1 | 2 |\n| --- | --- |\n| 3 | 4 |";
        let result = remove_mid_table_separators(input);
        // Only first separator preserved; second removed
        let sep_count = result.lines().filter(|l| is_separator_row(l)).count();
        assert_eq!(sep_count, 1, "Only one separator should remain");
        assert!(result.contains("| 3 | 4 |"), "Data rows should remain");
    }

    #[test]
    fn test_keep_only_header_separator() {
        let input = "| H1 | H2 |\n| --- | --- |\n| a | b |\n| c | d |";
        let result = remove_mid_table_separators(input);
        assert_eq!(result, input, "Normal table should be unchanged");
    }

    #[test]
    fn test_clean_markdown_full_pipeline() {
        let input = "```markdown\n# Title\r\n\r\nSome text   \n\n\n\n\n\n## Section\n\n| A | B |\n| 1 | 2 |\n```";
        let result = clean_markdown(input);
        assert!(result.starts_with("# Title"));
        assert!(result.ends_with('\n'));
        // No excessive blank lines
        assert!(!result.contains("\n\n\n\n"));
    }
}
