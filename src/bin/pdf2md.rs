//! CLI binary for edgequake-pdf2md.
//!
//! A thin shim over the library crate that maps CLI flags
//! to `ConversionConfig` and prints results.

use anyhow::{Context, Result};
use clap::Parser;
use edgequake_pdf2md::{
    convert, convert_to_file, inspect, ConversionConfig, ConversionProgressCallback, FidelityTier,
    PageSelection, PageSeparator, ProgressCallback,
};
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing_subscriber::EnvFilter;

// ── ANSI colour helpers (no extra deps) ──────────────────────────────────────

fn green(s: &str) -> String {
    format!("\x1b[32m{s}\x1b[0m")
}
fn red(s: &str) -> String {
    format!("\x1b[31m{s}\x1b[0m")
}
fn dim(s: &str) -> String {
    format!("\x1b[2m{s}\x1b[0m")
}
fn bold(s: &str) -> String {
    format!("\x1b[1m{s}\x1b[0m")
}
fn cyan(s: &str) -> String {
    format!("\x1b[36m{s}\x1b[0m")
}

// ── CLI progress callback using indicatif ────────────────────────────────────

/// Terminal progress callback: renders a live progress bar and per-page log
/// lines using [indicatif]. Designed to work correctly when pages complete
/// out-of-order (concurrent mode).
struct CliProgressCallback {
    /// The single progress bar anchored at the bottom of the terminal.
    bar: ProgressBar,
    /// Per-page wall-clock start times for elapsed reporting.
    start_times: Mutex<HashMap<usize, Instant>>,
    /// Count of pages that errored out.
    errors: AtomicUsize,
}

impl CliProgressCallback {
    /// Create a callback whose progress-bar length is set dynamically
    /// by `on_conversion_start` (called before any pages are processed).
    fn new_dynamic() -> Arc<Self> {
        let bar = ProgressBar::new(0); // length set in on_conversion_start

        // Initial style: spinner only (no counter until we know the total).
        let spinner_style = ProgressStyle::with_template("{spinner:.cyan} {prefix:.bold}  {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner())
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "⠿"]);

        bar.set_style(spinner_style);
        bar.set_prefix("Preparing");
        bar.set_message("Opening PDF…");
        bar.enable_steady_tick(Duration::from_millis(80));

        Arc::new(Self {
            bar,
            start_times: Mutex::new(HashMap::new()),
            errors: AtomicUsize::new(0),
        })
    }

    /// Switch to the full progress-bar style once we know `total`.
    fn activate_bar(&self, total: usize) {
        let progress_style = ProgressStyle::with_template(
            "{spinner:.cyan} {prefix:.bold}  \
             [{bar:42.green/238}] {pos:>3}/{len} pages  \
             ⏱ {elapsed_precise}  ETA {eta_precise}",
        )
        .unwrap_or_else(|_| ProgressStyle::default_bar())
        .progress_chars("█▉▊▋▌▍▎▏  ")
        .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "⠿"]);

        self.bar.set_length(total as u64);
        self.bar.set_style(progress_style);
        self.bar.set_prefix("Converting");
        self.bar.reset_eta();
    }
}

impl ConversionProgressCallback for CliProgressCallback {
    fn on_conversion_start(&self, total_pages: usize) {
        // Switch from spinner-only style to full progress bar now that we
        // know the actual page count.
        self.activate_bar(total_pages);
        self.bar.println(format!(
            "{} {}",
            cyan("◆"),
            bold(&format!("Starting conversion of {total_pages} pages…"))
        ));
    }

    fn on_page_start(&self, page_num: usize, _total: usize) {
        self.start_times
            .lock()
            .unwrap()
            .insert(page_num, Instant::now());
        self.bar.set_message(format!("page {page_num}"));
    }

    fn on_page_complete(&self, page_num: usize, total: usize, markdown_len: usize) {
        let elapsed_ms = self
            .start_times
            .lock()
            .unwrap()
            .remove(&page_num)
            .map(|t| t.elapsed().as_millis())
            .unwrap_or(0);

        self.bar.println(format!(
            "  {} Page {:>3}/{:<3}  {:<8}  {}",
            green("✓"),
            page_num,
            total,
            dim(&format!("{markdown_len:>5} chars")),
            dim(&format!("{:.1}s", elapsed_ms as f64 / 1000.0)),
        ));
        self.bar.inc(1);
    }

    fn on_page_error(&self, page_num: usize, total: usize, error: String) {
        let elapsed_ms = self
            .start_times
            .lock()
            .unwrap()
            .remove(&page_num)
            .map(|t| t.elapsed().as_millis())
            .unwrap_or(0);

        self.errors.fetch_add(1, Ordering::SeqCst);

        // Truncate very long error messages to keep output tidy.
        let msg = if error.len() > 80 {
            format!("{}\u{2026}", &error[..79])
        } else {
            error
        };

        self.bar.println(format!(
            "  {} Page {:>3}/{:<3}  {}  {}",
            red("✗"),
            page_num,
            total,
            red(&msg),
            dim(&format!("{:.1}s", elapsed_ms as f64 / 1000.0)),
        ));
        self.bar.inc(1);
    }

    fn on_conversion_complete(&self, total_pages: usize, success_count: usize) {
        let failed = total_pages.saturating_sub(success_count);
        self.bar.finish_and_clear();

        if failed == 0 {
            eprintln!(
                "{} {} pages converted successfully",
                green("✔"),
                bold(&success_count.to_string())
            );
        } else {
            eprintln!(
                "{} {}/{} pages converted  ({} failed)",
                if failed == total_pages {
                    red("✘")
                } else {
                    cyan("⚠")
                },
                bold(&success_count.to_string()),
                total_pages,
                red(&failed.to_string()),
            );
        }
    }
}

const AFTER_HELP: &str = r#"EXAMPLES:
  # Basic conversion (stdout)
  pdf2md document.pdf

  # Convert to file
  pdf2md document.pdf -o output.md

  # Specific pages, high fidelity
  pdf2md --pages 1-5 --fidelity tier3 paper.pdf -o paper.md

  # Use a specific model
  pdf2md --model gpt-4.1 --provider openai document.pdf

  # Convert from URL
  pdf2md https://arxiv.org/pdf/1706.03762 -o attention.md

  # Inspect PDF metadata (no API key needed)
  pdf2md --inspect-only document.pdf

  # Sequential mode for consistent formatting
  pdf2md --maintain-format --pages all book.pdf -o book.md

  # JSON output with metadata
  pdf2md --json --metadata document.pdf > output.json

SUPPORTED PROVIDERS & MODELS:
  Provider     Model                  Input $/1M  Output $/1M  Vision
  ─────────    ─────────────────────  ──────────  ───────────  ──────
  openai       gpt-4.1-nano (default) $0.10       $0.40        ✓
  openai       gpt-4.1-mini           $0.40       $1.60        ✓
  openai       gpt-4.1                $2.00       $8.00        ✓
  openai       gpt-4o                 $2.50       $10.00       ✓
  anthropic    claude-sonnet-4-20250514         $3.00       $15.00       ✓
  anthropic    claude-haiku-4-20250514          $0.80       $4.00        ✓
  gemini       gemini-2.0-flash       $0.10       $0.40        ✓
  gemini       gemini-2.5-pro         $1.25       $10.00       ✓
  ollama       llava, llama3.2-vision free        free         ✓

COST ESTIMATE (50-page document @ 150 DPI):
  ~1,500 input tokens/page × 50 pages = 75K input tokens
  ~800 output tokens/page × 50 pages = 40K output tokens

  gpt-4.1-nano:  ~$0.02 total
  gpt-4.1-mini:  ~$0.09 total
  gpt-4.1:       ~$0.47 total
  claude-sonnet-4-20250514: ~$0.83 total

ENVIRONMENT VARIABLES:
  OPENAI_API_KEY          OpenAI API key
  ANTHROPIC_API_KEY       Anthropic API key
  GEMINI_API_KEY          Google Gemini API key
  EDGEQUAKE_LLM_PROVIDER  Override provider (openai, anthropic, gemini, ollama)
  EDGEQUAKE_MODEL         Override model ID
  PDFIUM_LIB_PATH         Path to an existing libpdfium — skips auto-download
  PDFIUM_AUTO_CACHE_DIR   Override the default pdfium cache directory

SETUP:
  1. Set API key:     export OPENAI_API_KEY=sk-...
  2. Convert:         pdf2md document.pdf -o output.md

  PDFium (~30 MB) is downloaded automatically on first run and cached in
  ~/.cache/pdf2md/pdfium-7690/. No manual library setup is required.
  To use an existing pdfium copy: PDFIUM_LIB_PATH=/path/to/libpdfium pdf2md ...
"#;

/// Convert PDF files and URLs to Markdown using Vision LLMs.
#[derive(Parser, Debug)]
#[command(
    name = "pdf2md",
    version,
    about = "Convert PDF files and URLs to Markdown using Vision LLMs",
    long_about = "Convert PDF documents (local files or URLs) to clean, well-structured Markdown \
using Vision Language Models. Supports OpenAI, Anthropic, Google Gemini, Azure OpenAI, and \
any OpenAI-compatible endpoint (Ollama, vLLM, LiteLLM, etc.).",
    arg_required_else_help = true,
    color = clap::ColorChoice::Auto,
    after_long_help = AFTER_HELP
)]
struct Cli {
    /// Local PDF file path or HTTP/HTTPS URL.
    input: String,

    /// Write Markdown to this file instead of stdout.
    #[arg(short, long, env = "PDF2MD_OUTPUT")]
    output: Option<PathBuf>,

    /// LLM model ID (e.g. gpt-4.1-nano, gpt-4.1, claude-sonnet-4-20250514).
    #[arg(
        long,
        env = "EDGEQUAKE_MODEL",
        long_help = "Vision LLM model to use. Default: gpt-4.1-nano ($0.10/$0.40 per 1M tokens).\n\
          Popular choices: gpt-4.1-mini ($0.40/$1.60), gpt-4.1 ($2/$8), claude-sonnet-4-20250514 ($3/$15)."
    )]
    model: Option<String>,

    /// LLM provider: openai, anthropic, gemini, ollama, azure.
    #[arg(
        long,
        env = "EDGEQUAKE_PROVIDER",
        long_help = "LLM provider. Auto-detected from API key env vars if not set.\n\
          Supported: openai, anthropic, gemini, azure, ollama, or any OpenAI-compatible URL."
    )]
    provider: Option<String>,

    /// Rendering DPI (72–400).
    #[arg(long, env = "PDF2MD_DPI", default_value_t = 150,
          value_parser = clap::value_parser!(u32).range(72..=400))]
    dpi: u32,

    /// Number of concurrent VLM API calls.
    #[arg(short, long, env = "PDF2MD_CONCURRENCY", default_value_t = 10)]
    concurrency: usize,

    /// Sequential mode: pass previous page as context for format continuity.
    #[arg(long, env = "PDF2MD_MAINTAIN_FORMAT")]
    maintain_format: bool,

    /// Page selection: all, 5, 3-15, or 1,3,5,7.
    #[arg(long, env = "PDF2MD_PAGES", default_value = "all")]
    pages: String,

    /// Output quality: tier1, tier2, tier3.
    #[arg(long, env = "PDF2MD_FIDELITY", value_enum, default_value = "tier2")]
    fidelity: FidelityArg,

    /// Page separator: none, hr, comment, or custom string.
    #[arg(long, env = "PDF2MD_SEPARATOR", default_value = "none")]
    separator: String,

    /// PDF user password for encrypted documents.
    #[arg(long, env = "PDF2MD_PASSWORD")]
    password: Option<String>,

    /// Path to a text file containing a custom system prompt.
    #[arg(long, env = "PDF2MD_SYSTEM_PROMPT")]
    system_prompt: Option<PathBuf>,

    /// Max LLM output tokens per page.
    #[arg(long, env = "PDF2MD_MAX_TOKENS", default_value_t = 4096)]
    max_tokens: usize,

    /// LLM temperature (0.0–2.0).
    #[arg(long, env = "PDF2MD_TEMPERATURE", default_value_t = 0.1)]
    temperature: f32,

    /// Retries per page on LLM failure.
    #[arg(long, env = "PDF2MD_MAX_RETRIES", default_value_t = 3)]
    max_retries: u32,

    /// Prepend YAML front-matter with document metadata.
    #[arg(long, env = "PDF2MD_METADATA")]
    metadata: bool,

    /// Output structured JSON (ConversionOutput) instead of Markdown.
    #[arg(long, env = "PDF2MD_JSON")]
    json: bool,

    /// Disable progress bar.
    #[arg(long, env = "PDF2MD_NO_PROGRESS")]
    no_progress: bool,

    /// Print PDF metadata only, no conversion.
    #[arg(long)]
    inspect_only: bool,

    /// Enable DEBUG-level tracing logs.
    #[arg(short, long, env = "PDF2MD_VERBOSE")]
    verbose: bool,

    /// Suppress all output except errors.
    #[arg(short, long, env = "PDF2MD_QUIET")]
    quiet: bool,

    /// HTTP download timeout in seconds.
    #[arg(long, env = "PDF2MD_DOWNLOAD_TIMEOUT", default_value_t = 120)]
    download_timeout: u64,

    /// Per-page LLM call timeout in seconds.
    #[arg(long, env = "PDF2MD_API_TIMEOUT", default_value_t = 60)]
    api_timeout: u64,
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum FidelityArg {
    Tier1,
    Tier2,
    Tier3,
}

impl From<FidelityArg> for FidelityTier {
    fn from(v: FidelityArg) -> Self {
        match v {
            FidelityArg::Tier1 => FidelityTier::Tier1,
            FidelityArg::Tier2 => FidelityTier::Tier2,
            FidelityArg::Tier3 => FidelityTier::Tier3,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // ── Logging setup ────────────────────────────────────────────────────
    // Suppress INFO-level library logs when the progress bar is active;
    // the bar provides all the feedback that matters to the user.
    let show_progress = !cli.quiet && !cli.no_progress && !cli.json;
    let filter = if cli.quiet || show_progress {
        "error"
    } else if cli.verbose {
        "debug"
    } else {
        "info"
    };
    // In verbose mode we always want all logs regardless of progress.
    let filter = if cli.verbose { "debug" } else { filter };

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter)),
        )
        .with_writer(io::stderr)
        .init();

    // ── Ensure PDFium engine is available ───────────────────────────────────
    // When compiled with `--features bundled`, the pdfium shared library was
    // embedded at compile time.  We just extract it (if needed) and continue.
    // Without `bundled`, on the very first run pdf2md downloads the library
    // (~30 MB) from bblanchon/pdfium-binaries to
    //   ~/.cache/pdf2md/pdfium-{VERSION}/
    // Subsequent startups skip this block entirely (instant path check only).
    #[cfg(feature = "bundled")]
    {
        tokio::task::block_in_place(|| pdfium_auto::ensure_pdfium_bundled())
            .context("Failed to extract bundled PDFium engine")?;
    }

    #[cfg(not(feature = "bundled"))]
    if !pdfium_auto::is_pdfium_cached() {
        if !cli.quiet {
            let dl_bar = ProgressBar::new(0);
            dl_bar.set_style(
                ProgressStyle::with_template(
                    "{spinner:.cyan} {prefix:.bold}  \
                     [{bar:42.green/238}] {bytes}/{total_bytes}  ETA {eta_precise}",
                )
                .unwrap_or_else(|_| ProgressStyle::default_bar())
                .progress_chars("█▉▊▋▌▍▎▏  ")
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "⠿"]),
            );
            dl_bar.set_prefix("PDF engine");
            dl_bar.set_message("Connecting…");
            dl_bar.enable_steady_tick(Duration::from_millis(80));

            let bar = dl_bar.clone();
            // block_in_place keeps the reference lifetime valid (no 'static
            // requirement) while still offloading the blocking download from
            // the async executor's hot path.
            tokio::task::block_in_place(|| {
                pdfium_auto::ensure_pdfium_library(Some(&|downloaded, total| {
                    if let Some(t) = total {
                        if bar.length().unwrap_or(0) != t {
                            bar.set_length(t);
                            bar.set_prefix("PDF engine");
                        }
                        bar.set_position(downloaded);
                    } else {
                        bar.set_position(downloaded);
                    }
                }))
            })
            .context("Failed to download PDFium engine")?;

            dl_bar.finish_with_message("ready ✓");
        } else {
            // Quiet mode — download silently; errors still propagate.
            tokio::task::block_in_place(|| pdfium_auto::ensure_pdfium_library(None))
                .context("Failed to download PDFium engine")?;
        }
    }

    // ── Inspect-only mode ────────────────────────────────────────────────
    if cli.inspect_only {
        let meta = inspect(&cli.input).await.context("Failed to inspect PDF")?;

        if cli.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&meta).context("Failed to serialize metadata")?
            );
        } else {
            println!("File:         {}", cli.input);
            if let Some(ref t) = meta.title {
                println!("Title:        {}", t);
            }
            if let Some(ref a) = meta.author {
                println!("Author:       {}", a);
            }
            if let Some(ref s) = meta.subject {
                println!("Subject:      {}", s);
            }
            println!("Pages:        {}", meta.page_count);
            println!("PDF Version:  {}", meta.pdf_version);
            println!("Encrypted:    {}", meta.is_encrypted);
            if let Some(ref p) = meta.producer {
                println!("Producer:     {}", p);
            }
            if let Some(ref c) = meta.creator {
                println!("Creator:      {}", c);
            }
        }
        return Ok(());
    }

    // ── Build config ─────────────────────────────────────────────────────
    // The progress bar is initialised with a spinner (no page count yet);
    // `on_conversion_start` resizes it to the correct total once the PDF
    // has been inspected. `show_progress` was already computed above.

    let progress_cb: Option<ProgressCallback> = if show_progress {
        let cb = CliProgressCallback::new_dynamic();
        Some(cb as Arc<dyn ConversionProgressCallback>)
    } else {
        None
    };

    let config = build_config(&cli, progress_cb).await?;

    // ── Run conversion ───────────────────────────────────────────────────
    if let Some(ref output_path) = cli.output {
        let stats = convert_to_file(&cli.input, output_path, &config)
            .await
            .context("Conversion failed")?;

        // Summary line (callback already printed the per-page log).
        if !cli.quiet {
            let selected = stats.processed_pages + stats.failed_pages + stats.skipped_pages;
            eprintln!(
                "{}  {}/{} pages  {}ms  →  {}",
                if stats.failed_pages == 0 {
                    green("✔")
                } else {
                    cyan("⚠")
                },
                stats.processed_pages,
                selected,
                stats.total_duration_ms,
                bold(&output_path.display().to_string()),
            );
            eprintln!(
                "   {} tokens in  /  {} tokens out",
                dim(&stats.total_input_tokens.to_string()),
                dim(&stats.total_output_tokens.to_string()),
            );
        }
    } else {
        let output = convert(&cli.input, &config)
            .await
            .context("Conversion failed")?;

        if cli.json {
            let json =
                serde_json::to_string_pretty(&output).context("Failed to serialise output")?;
            println!("{json}");
        } else {
            let stdout = io::stdout();
            let mut handle = stdout.lock();
            handle
                .write_all(output.markdown.as_bytes())
                .context("Failed to write to stdout")?;
            // Ensure a trailing newline on stdout.
            if !output.markdown.ends_with('\n') {
                handle.write_all(b"\n").ok();
            }
        }

        // Summary (the callback already printed the final green/red tick).
        if !cli.quiet && !show_progress {
            // Only print inline stats when the progress callback is disabled.
            let selected = output.stats.processed_pages
                + output.stats.failed_pages
                + output.stats.skipped_pages;
            eprintln!(
                "Converted {}/{} pages in {}ms",
                output.stats.processed_pages, selected, output.stats.total_duration_ms
            );
            if output.stats.failed_pages > 0 {
                eprintln!("  {} pages failed", output.stats.failed_pages);
            }
        } else if !cli.quiet && !cli.json {
            eprintln!(
                "   {} tokens in  /  {} tokens out  —  {}ms total",
                dim(&output.stats.total_input_tokens.to_string()),
                dim(&output.stats.total_output_tokens.to_string()),
                output.stats.total_duration_ms,
            );
        }
    }

    Ok(())
}

/// Map CLI args to `ConversionConfig`.
async fn build_config(cli: &Cli, progress: Option<ProgressCallback>) -> Result<ConversionConfig> {
    let system_prompt = if let Some(ref path) = cli.system_prompt {
        Some(
            tokio::fs::read_to_string(path)
                .await
                .with_context(|| format!("Failed to read system prompt from {:?}", path))?,
        )
    } else {
        None
    };

    let pages = parse_pages(&cli.pages)?;
    let separator = parse_separator(&cli.separator);

    let mut builder = ConversionConfig::builder()
        .dpi(cli.dpi)
        .concurrency(cli.concurrency)
        .maintain_format(cli.maintain_format)
        .pages(pages)
        .fidelity(cli.fidelity.clone().into())
        .page_separator(separator)
        .max_tokens(cli.max_tokens)
        .temperature(cli.temperature)
        .max_retries(cli.max_retries)
        .include_metadata(cli.metadata)
        .download_timeout_secs(cli.download_timeout)
        .api_timeout_secs(cli.api_timeout);

    if let Some(cb) = progress {
        builder = builder.progress_callback(cb);
    }

    let mut config = builder.build().context("Invalid configuration")?;

    // Apply fields the builder doesn't have setters for (or that need special handling)
    config.model = cli.model.clone();
    config.provider_name = cli.provider.clone();
    config.password = cli.password.clone();
    config.system_prompt = system_prompt;

    Ok(config)
}

/// Parse `--pages` string into `PageSelection`.
fn parse_pages(s: &str) -> Result<PageSelection> {
    let s = s.trim().to_lowercase();

    if s == "all" {
        return Ok(PageSelection::All);
    }

    // Range: "3-15"
    if let Some((start, end)) = s.split_once('-') {
        let start: usize = start
            .trim()
            .parse()
            .context("Invalid start page in range")?;
        let end: usize = end.trim().parse().context("Invalid end page in range")?;

        if start < 1 {
            anyhow::bail!("Pages are 1-indexed, minimum is 1 (got {})", start);
        }
        if start > end {
            anyhow::bail!(
                "Invalid page range '{}-{}': start must be <= end",
                start,
                end
            );
        }

        return Ok(PageSelection::Range(start, end));
    }

    // Set: "1,3,5,7"
    if s.contains(',') {
        let pages: Vec<usize> = s
            .split(',')
            .map(|p| {
                p.trim()
                    .parse::<usize>()
                    .context(format!("Invalid page number: '{}'", p.trim()))
            })
            .collect::<Result<Vec<_>>>()?;

        for &p in &pages {
            if p < 1 {
                anyhow::bail!("Pages are 1-indexed, minimum is 1 (got {})", p);
            }
        }

        return Ok(PageSelection::Set(pages));
    }

    // Single page: "5"
    let page: usize = s.parse().context("Invalid page number")?;
    if page < 1 {
        anyhow::bail!("Pages are 1-indexed, minimum is 1 (got {})", page);
    }

    Ok(PageSelection::Single(page))
}

/// Parse `--separator` string into `PageSeparator`.
fn parse_separator(s: &str) -> PageSeparator {
    match s.to_lowercase().as_str() {
        "none" => PageSeparator::None,
        "hr" | "---" => PageSeparator::HorizontalRule,
        "comment" => PageSeparator::Comment,
        custom => PageSeparator::Custom(custom.to_string()),
    }
}
