# Contributing to edgequake-pdf2md

Thank you for considering contributing to edgequake-pdf2md! This document outlines our CI/CD standards, development workflow, and pre-release checks.

## Table of Contents

- [Development Setup](#development-setup)
- [Local CI Checks](#local-ci-checks)
- [Pre-Commit Hooks](#pre-commit-hooks)
- [Testing](#testing)
- [Documentation](#documentation)
- [Code Quality Standards](#code-quality-standards)
- [Publishing](#publishing)
- [CI/CD Workflow](#cicd-workflow)

## Development Setup

### Prerequisites

- Rust 1.80+ ([install](https://www.rust-lang.org/tools/install))
- macOS/Linux (primary development targets)
- pdfium library (auto-installed with `make setup`)

### Initial Setup

```bash
git clone https://github.com/raphaelmansuy/edgequake-pdf2md
cd edgequake-pdf2md
make setup          # Download pdfium and verify environment
make build          # Build release binary
make test           # Run unit tests
make ci              # Run all CI checks
```

## Local CI Checks

Before committing or pushing, run these checks locally to match what the CI/CD pipeline will do.

### Quick CI Check

```bash
make ci              # Format check + Lint + Unit tests + Doc tests
```

### Individual Checks

#### 1. Code Formatting (Required)

```bash
# Check formatting without modifying files
make fmt-check

# Auto-fix formatting issues
make fmt
```

**What it does:** Ensures code follows Rust style guidelines using `rustfmt`.

**Most common issue:** Line length violations. `rustfmt` will reformat long lines automatically.

---

#### 2. Linting with Clippy (Required)

```bash
make lint
```

**What it does:** Runs static analysis to catch common mistakes and non-idiomatic code.

**Strictness:** We use `-D warnings`, which means all warnings fail the check.

**Example fixes:**
- Question mark operator instead of `match`/`if let`
- Iterators instead of manual loops
- Unnecessary clones or allocations

---

#### 3. Unit Tests (Required)

```bash
make test            # Run all unit tests
make doc-test        # Run documentation examples
make test-all        # Unit + E2E tests (needs API key)
```

**What it does:** Executes test cases and ensures nothing is broken.

**Test location:** `src/` (inline `#[cfg(test)]` modules) and `tests/e2e.rs`

---

#### 4. Build in Release Mode (Required)

```bash
make build           # Full release build
cargo build --release --features cli
```

**What it does:** Compiles the project with optimizations enabled.

**Ensures:** All code paths compile without errors or warnings.

---

#### 5. Documentation (Required)

```bash
# Build and check docs compile without warnings
cargo doc --no-deps --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
```

**What it does:** Generates API documentation and tests code examples in doc comments.

**Important:** All doc examples in comments must compile and run successfully.

---

#### 6. Security Audit (Recommended)

```bash
make audit
```

**What it does:** Checks for known security vulnerabilities in dependencies.

**Note:** This requires `cargo-audit` (installed automatically by `make audit`).

---

#### 7. MSRV Check (Required in CI)

```bash
make msrv            # Verify code works on Rust 1.80
```

**What it does:** Ensures the project compiles on the MSRV (Minimum Supported Rust Version).

**Current MSRV:** Rust 1.80 (specified in `Cargo.toml`)

## Pre-Commit Hooks

### Installation

```bash
# Install pre-commit framework (requires Python)
pip install pre-commit

# Install the git hooks
cd /path/to/edgequake-pdf2md
pre-commit install

# Verify installation
pre-commit run --all-files
```

### What Gets Checked Automatically

When you try to commit, pre-commit will automatically:

1. **Format code** with `rustfmt`
2. **Run clippy** linter
3. **Check YAML/JSON/TOML** syntax
4. **Trim trailing whitespace**
5. **Ensure files end with newline**
6. **Prevent large files** (>1MB)
7. **Detect private keys**

### Skipping Hooks (Not Recommended)

```bash
# Skip pre-commit checks (use sparingly!)
git commit --no-verify
```

## Testing

### Unit Tests

```bash
# Run all unit tests
cargo test --lib

# Run tests in a specific file
cargo test --lib pipeline::postprocess

# Run a single test
cargo test --lib pipeline::postprocess::tests::test_clean_markdown_full_pipeline

# Run with output
cargo test --lib -- --nocapture
```

### Documentation Tests

```bash
# Test all code examples in documentation
cargo test --doc
```

### End-to-End Tests

```bash
# Run e2e tests (requires test PDFs + API key)
make test-e2e

# With verbose output
make test-e2e-verbose
```

**Setup:**
1. Ensure test PDFs are downloaded: `make download-test-pdfs`
2. Set an LLM API key:
   ```bash
   export OPENAI_API_KEY="sk-..."
   # OR
   export ANTHROPIC_AUTH_TOKEN="..."
   ```
3. Enable E2E tests: `E2E_ENABLED=1 cargo test --test e2e -- --nocapture`

## Documentation

### Building Documentation

```bash
# Build and open documentation
cargo doc --no-deps --open

# Build with strict warnings treated as errors
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
```

### Document Your Code

Add doc comments to public items:

```rust
/// Converts a PDF to Markdown using a vision LLM.
///
/// # Arguments
///
/// * `input` - Path to PDF file or URL
/// * `config` - Conversion configuration
///
/// # Example
///
/// ```
/// use edgequake_pdf2md::{convert, ConversionConfig};
///
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// let md = convert("input.pdf", Config::default()).await.unwrap();
/// # });
/// ```
pub async fn convert(input: impl AsRef<str>, config: ConversionConfig) -> Result<String> {
    // implementation
}
```

## Code Quality Standards

### Required Standards

| Check | Tool | Command | Must Pass |
|-------|------|---------|-----------|
| Formatting | `rustfmt` | `make fmt-check` | ✅ YES |
| Linting | `clippy` | `make lint` | ✅ YES |
| Unit Tests | `cargo test` | `make test` | ✅ YES |
| Doc Tests | `cargo test --doc` | `make doc-test` | ✅ YES |
| Build | `cargo build` | `make build` | ✅ YES |
| Documentation | `cargo doc` | Generated successfully | ✅ YES |
| MSRV | `cargo +1.80` | `make msrv` | ✅ YES (in CI) |

### Optional but Recommended

- Security audit: `make audit`
- Full E2E test suite: `make test-all` (requires API key)

## Publishing

### Pre-Publish Checklist

Before publishing a new version, run the comprehensive pre-publish script:

```bash
./scripts/pre-publish-check.sh
```

This script verifies:

1. ✓ Formatting is correct
2. ✓ Clippy passes
3. ✓ All unit tests pass
4. ✓ Documentation builds without warnings
5. ✓ Security audit passes
6. ✓ Release build succeeds
7. ✓ README.md exists
8. ✓ LICENSE exists
9. ✓ CHANGELOG.md exists
10. ✓ Version in Cargo.toml matches tag

### Manual Version Verification

```bash
# Check version in Cargo.toml
grep '^version = ' Cargo.toml

# Verify version format
./scripts/pre-publish-check.sh --version 0.2.0
```

### Creating a Release

1. **Update version** in `Cargo.toml`:
   ```toml
   [package]
   version = "0.2.0"  # Updated from 0.1.0
   ```

2. **Update CHANGELOG.md** with release notes:
   ```markdown
   ## [0.2.0] - 2026-02-19
   
   ### Added
   - New feature X
   
   ### Fixed
   - Bug fix Y
   ```

3. **Run pre-publish checks**:
   ```bash
   ./scripts/pre-publish-check.sh --version 0.2.0
   ```

4. **Commit the changes**:
   ```bash
   git add Cargo.toml CHANGELOG.md
   git commit -m "chore: release v0.2.0"
   ```

5. **Create a git tag**:
   ```bash
   git tag v0.2.0
   git push origin main v0.2.0
   ```

6. **GitHub Actions** `publish.yml` will automatically:
   - Run comprehensive pre-publish checks
   - Publish to [crates.io](https://crates.io/crates/edgequake-pdf2md)
   - Create a GitHub Release

### Publishing Manually

If the automated workflow doesn't trigger:

```bash
# Ensure all checks pass
./scripts/pre-publish-check.sh --version 0.2.0

# Publish to crates.io
cargo publish --token $CARGO_REGISTRY_TOKEN
```

**Note:** You need a crates.io account and API token.

## CI/CD Workflow

### GitHub Actions Workflows

#### 1. CI Pipeline (`ci.yml`)

**Triggers:** Every push to `main`/`develop`, all pull requests

**Jobs:**
- `format` — Formatting check
- `lint` — Clippy linter
- `test` — Unit tests
- `build` — Release build
- `security-audit` — Vulnerability scan
- `docs` — Documentation build
- `msrv` — MSRV compatibility
- `ci-status` — Final status aggregator

**Time:** ~2 minutes (parallelized)

#### 2. Publish Pipeline (`publish.yml`)

**Triggers:** Push tag matching `v*.*.*` (e.g., `v0.2.0`)

**Jobs:**
- `pre-publish-checks` — Comprehensive pre-release validation
- `publish` — Publish to crates.io
- `release` — Create GitHub Release

**Time:** ~3 minutes

### Viewing Workflow Status

- **GitHub:** Actions tab → Select workflow → Latest run
- **Local:** None (workflows run on GitHub)

### Understanding Failures

Most CI failures fall into these categories:

1. **Format failures** → Run `make fmt`
2. **Lint failures** → Review clippy suggestions, fix code
3. **Test failures** → Check test output, debug locally
4. **Build failures** → Compilation error, check compiler message
5. **Doc failures** → Fix doc comments or examples

### Debugging CI

1. **Run checks locally first** before pushing:
   ```bash
   make ci-all
   ```

2. **If a check passes locally but fails in CI:**
   - Check Rust version: `cargo --version`
   - Wipe build cache: `cargo clean`
   - Rebuild: `cargo build`

3. **Check GitHub Actions logs:**
   - Go to GitHub → Actions → Failed workflow
   - Expand failed job to see full output

## Questions?

- Open an issue: [GitHub Issues](https://github.com/raphaelmansuy/edgequake-pdf2md/issues)
- Check docs: [Docs](https://docs.rs/edgequake-pdf2md)

## Summary

**Before committing:**
```bash
make fmt-check lint test doc-test
```

**Before pushing:**
```bash
make ci-all
```

**Before publishing:**
```bash
./scripts/pre-publish-check.sh --version X.Y.Z
```

Thank you for maintaining high code quality! 🎉
