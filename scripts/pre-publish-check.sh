#!/usr/bin/env bash
# ==============================================================================
# Pre-Publish Check Script for edgequake-pdf2md
# ==============================================================================
# This script runs comprehensive checks before publishing a new version.
# 
# Usage:
#   ./scripts/pre-publish-check.sh [--version VERSION]
#   ./scripts/pre-publish-check.sh --help
#
# Examples:
#   # Check in dry-run mode
#   ./scripts/pre-publish-check.sh
#
#   # Check with specific version
#   ./scripts/pre-publish-check.sh --version 0.2.0
#
# ==============================================================================

set -euo pipefail

# ── Configuration ──────────────────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
CARGO_TOML="$PROJECT_ROOT/Cargo.toml"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
RESET='\033[0m'

# Tracking
CHECKS_PASSED=0
CHECKS_FAILED=0
WARNINGS_COUNT=0

# ── Functions ──────────────────────────────────────────────────────────────

print_header() {
    printf "\n${BOLD}${CYAN}▸ %s${RESET}\n" "$1"
}

print_success() {
    printf "${GREEN}✓${RESET} %s\n" "$1"
    ((CHECKS_PASSED++))
}

print_failure() {
    printf "${RED}✗${RESET} %s\n" "$1"
    ((CHECKS_FAILED++))
}

print_warning() {
    printf "${YELLOW}⚠${RESET} %s\n" "$1"
    ((WARNINGS_COUNT++))
}

print_info() {
    printf "  %s\n" "$1"
}

show_help() {
    cat << EOF
${BOLD}Pre-Publish Check Script${RESET}

${BOLD}Usage:${RESET}
  ./scripts/pre-publish-check.sh [OPTIONS]

${BOLD}Options:${RESET}
  --version VERSION    Verify this version matches Cargo.toml
  --help              Show this help message

${BOLD}Examples:${RESET}
  ./scripts/pre-publish-check.sh
  ./scripts/pre-publish-check.sh --version 0.2.0

EOF
    exit 0
}

check_command() {
    if ! command -v "$1" &> /dev/null; then
        print_failure "Command not found: $1"
        return 1
    fi
    return 0
}

get_cargo_version() {
    grep '^version = ' "$CARGO_TOML" | head -1 | sed 's/version = "//' | sed 's/".*//'
}

# ── Parse Arguments ────────────────────────────────────────────────────────

VERIFY_VERSION=""
while [[ $# -gt 0 ]]; do
    case $1 in
        --version)
            VERIFY_VERSION="$2"
            shift 2
            ;;
        --help)
            show_help
            ;;
        *)
            printf "${RED}Unknown option: $1${RESET}\n" >&2
            show_help
            ;;
    esac
done

# ── Main Checks ────────────────────────────────────────────────────────────

printf "${BOLD}${CYAN}═══════════════════════════════════════════════════════════${RESET}\n"
printf "${BOLD}  edgequake-pdf2md — Pre-Publish Checks${RESET}\n"
printf "${BOLD}${CYAN}═══════════════════════════════════════════════════════════${RESET}\n"

cd "$PROJECT_ROOT"

# 1. Check tools are available
print_header "Environment Setup"

if check_command "cargo" && check_command "rustc"; then
    print_success "Rust toolchain available"
else
    print_failure "Rust toolchain not found (install from https://rustup.rs)"
    exit 1
fi

# 2. Version verification
print_header "Version Check"

CARGO_VERSION=$(get_cargo_version)
print_info "Cargo.toml version: $CARGO_VERSION"

if [ -n "$VERIFY_VERSION" ]; then
    # Remove 'v' prefix if present
    VERIFY_VERSION_NO_V="${VERIFY_VERSION#v}"
    if [ "$VERIFY_VERSION_NO_V" = "$CARGO_VERSION" ]; then
        print_success "Version matches: $VERIFY_VERSION_NO_V"
    else
        print_failure "Version mismatch!"
        print_info "Expected: $VERIFY_VERSION_NO_V"
        print_info "Found:    $CARGO_VERSION"
        exit 1
    fi
fi

# 3. Format check
print_header "Code Formatting"

if cargo fmt --all -- --check 2>&1 | grep -q "Diff in" || cargo fmt --all -- --check 2>&1 | grep -q "error\|diff"; then
    print_failure "Formatting issues found!"
    print_info "Run: cargo fmt --all"
    exit 1
else
    print_success "Code is properly formatted"
fi

# 4. Clippy lint
print_header "Linting (Clippy)"

if cargo clippy --all-features -- -D warnings 2>&1 | tail -1 | grep -q "Finished"; then
    print_success "No clippy warnings"
else
    print_failure "Clippy errors found"
    exit 1
fi

# 5. Unit tests
print_header "Unit Tests"

if cargo test --lib --no-fail-fast 2>&1 | tail -3 | grep -q "test result: ok"; then
    print_success "All unit tests pass"
else
    print_failure "Some tests failed"
    exit 1
fi

# 6. Build release
print_header "Build Release"

if cargo build --release --all-features 2>&1 | tail -1 | grep -q "Finished"; then
    print_success "Release build successful"
else
    print_failure "Release build failed"
    exit 1
fi

# 7. Documentation
print_header "Documentation"

if RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features 2>&1 | tail -1 | grep -q "Finished"; then
    print_success "Documentation builds without warnings"
else
    print_failure "Documentation errors found"
    exit 1
fi

# 8. Run doctests
if cargo test --doc 2>&1 | tail -3 | grep -q "test result: ok"; then
    print_success "Doc tests pass"
else
    print_failure "Doc tests failed"
    exit 1
fi

# 9. Security audit
print_header "Security Audit"

if command -v cargo-audit &> /dev/null; then
    if cargo audit 2>&1 | grep -q "no vulnerabilities detected"; then
        print_success "No known vulnerabilities"
    else
        print_warning "Vulnerabilities found - review before publishing"
    fi
else
    print_warning "cargo-audit not installed (run: cargo install cargo-audit)"
fi

# 10. Check required files
print_header "Required Files"

if [ -f "README.md" ]; then
    print_success "README.md exists"
else
    print_failure "README.md is missing"
    exit 1
fi

if [ -f "LICENSE" ]; then
    print_success "LICENSE exists"
else
    print_failure "LICENSE is missing"
    exit 1
fi

if [ -f "CHANGELOG.md" ]; then
    print_success "CHANGELOG.md exists"
else
    print_warning "CHANGELOG.md not found (recommended)"
fi

# 11. Git status
print_header "Git Status"

if [ -d ".git" ]; then
    if git diff-index --quiet HEAD -- &>/dev/null; then
        print_success "Working directory is clean"
    else
        print_warning "Uncommitted changes in working directory"
        print_info "Commit or stash changes before publishing"
    fi
else
    print_info "Not a git repository"
fi

# 12. Crate metadata
print_header "Crate Metadata"

CRATE_NAME=$(grep '^name = ' "$CARGO_TOML" | head -1 | sed 's/name = "//' | sed 's/".*//')
CRATE_DESC=$(grep '^description = ' "$CARGO_TOML" | head -1 | sed 's/description = "//' | sed 's/".*//')

print_info "Crate: $CRATE_NAME"
print_info "Version: $CARGO_VERSION"
print_info "Description: $CRATE_DESC"

# ── Summary ────────────────────────────────────────────────────────────────

printf "\n${BOLD}${CYAN}═══════════════════════════════════════════════════════════${RESET}\n"
printf "${BOLD}Summary${RESET}\n"
printf "${BOLD}${CYAN}═══════════════════════════════════════════════════════════${RESET}\n"

print_info "Checks passed: ${GREEN}${CHECKS_PASSED}${RESET}"
if [ $CHECKS_FAILED -gt 0 ]; then
    print_info "Checks failed: ${RED}${CHECKS_FAILED}${RESET}"
fi
if [ $WARNINGS_COUNT -gt 0 ]; then
    print_info "Warnings: ${YELLOW}${WARNINGS_COUNT}${RESET}"
fi

if [ $CHECKS_FAILED -eq 0 ]; then
    printf "\n${GREEN}${BOLD}✓ All checks passed! Ready to publish.${RESET}\n\n"
    cat << 'EOF'
${BOLD}Next steps:${RESET}

  1. Create a git tag:
     git tag v0.X.Y
     git push origin v0.X.Y

  2. Or trigger manual workflow dispatch on GitHub

  3. Monitor the publish workflow at:
     https://github.com/raphaelmansuy/edgequake-pdf2md/actions

EOF
    exit 0
else
    printf "\n${RED}${BOLD}✗ Some checks failed. Please fix before publishing.${RESET}\n\n"
    exit 1
fi
