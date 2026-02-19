# ==============================================================================
# edgequake-pdf2md — Developer Makefile
# ==============================================================================
# Usage:  make <target>
#
# Targets are grouped by purpose and listed in `make help` (default).
# The DYLD_LIBRARY_PATH variable is set automatically so that libpdfium.dylib
# in the project root is found by both the dev and release binaries.
# ==============================================================================

# ── Configuration ──────────────────────────────────────────────────────────────
SHELL        := /bin/bash
.DEFAULT_GOAL := help

ROOT_DIR     := $(dir $(abspath $(lastword $(MAKEFILE_LIST))))
PDFIUM_LIB   := $(ROOT_DIR)libpdfium.dylib
RELEASE_BIN  := $(ROOT_DIR)target/release/pdf2md
DEBUG_BIN    := $(ROOT_DIR)target/debug/pdf2md
TEST_DIR     := $(ROOT_DIR)test_cases
OUT_DIR      := $(ROOT_DIR)test_cases/output

# Detect the running binary (prefer debug for `make run`, release for `make demo`)
BIN          ?= $(RELEASE_BIN)

# Test PDFs
PDF_ARXIV    := $(TEST_DIR)/attention_is_all_you_need.pdf
PDF_IRS      := $(TEST_DIR)/irs_form_1040.pdf
PDF_NEURO    := $(TEST_DIR)/neuroscience_textbook.pdf
PDF_TEXT     := $(TEST_DIR)/sample_text.pdf

# Colours for terminal output
BOLD  := \033[1m
GREEN := \033[0;32m
CYAN  := \033[0;36m
YELLOW:= \033[0;33m
RED   := \033[0;31m
RESET := \033[0m

# Helper: run pdf2md with the library path and provider pre-set
PDF2MD := DYLD_LIBRARY_PATH=$(ROOT_DIR) EDGEQUAKE_LLM_PROVIDER=openai EDGEQUAKE_MODEL=gpt-4.1-nano $(BIN)

# ==============================================================================
# ── Help ───────────────────────────────────────────────────────────────────────
# ==============================================================================

.PHONY: help
help: ## Show this help message
	@printf "$(BOLD)edgequake-pdf2md — Developer Makefile$(RESET)\n\n"
	@printf "$(CYAN)Build targets:$(RESET)\n"
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
	  awk 'BEGIN {FS = ":.*?## "}; /^[a-zA-Z]/ {printf "  $(GREEN)%-22s$(RESET) %s\n", $$1, $$2}' | \
	  sort
	@printf "\n$(CYAN)Environment variables:$(RESET)\n"
	@printf "  $(YELLOW)EDGEQUAKE_PROVIDER$(RESET)   LLM provider  (e.g. openai, anthropic, gemini)\n"
	@printf "  $(YELLOW)EDGEQUAKE_MODEL$(RESET)      LLM model ID  (default: gpt-4.1-nano)\n"
	@printf "  $(YELLOW)PDF2MD_PAGES$(RESET)         Pages to convert (all|N|M-N|N,M,...)\n"
	@printf "  $(YELLOW)BIN$(RESET)                  Path to pdf2md binary (default: release)\n"
	@printf "\n$(CYAN)Quick start:$(RESET)\n"
	@printf "  make setup           # Download pdfium + check env\n"
	@printf "  make build           # Build release binary\n"
	@printf "  make ci              # Run CI checks (format + lint + test + docs)\n"
	@printf "  make pre-publish     # Run comprehensive pre-publish checks\n"
	@printf "  make demo            # Convert page 1 of Attention paper\n"
	@printf "  make test-e2e        # Run all e2e tests (needs API key)\n\n"

# ==============================================================================
# ── Setup / Bootstrap ──────────────────────────────────────────────────────────
# ==============================================================================

.PHONY: setup
setup: check-pdfium check-api-key ## Check pdfium library and API key are present

.PHONY: check-pdfium
check-pdfium: ## Verify pdfium library exists (auto-downloads if missing)
	@if [ ! -f "$(PDFIUM_LIB)" ]; then \
	  printf "$(YELLOW)pdfium library not found — running setup-pdfium.sh...$(RESET)\n"; \
	  bash $(ROOT_DIR)scripts/setup-pdfium.sh --install-dir $(ROOT_DIR); \
	else \
	  printf "$(GREEN)✓ pdfium library present ($(shell ls -lh $(PDFIUM_LIB) | awk '{print $$5}'))$(RESET)\n"; \
	fi

.PHONY: check-api-key
check-api-key: ## Verify at least one LLM API key is set
	@if [ -z "$$OPENAI_API_KEY" ] && [ -z "$$ANTHROPIC_AUTH_TOKEN" ] && [ -z "$$GEMINI_API_KEY" ]; then \
	  printf "$(RED)✗ No API key found$(RESET)\n"; \
	  printf "  Set OPENAI_API_KEY, ANTHROPIC_AUTH_TOKEN, or GEMINI_API_KEY\n"; \
	  exit 1; \
	else \
	  printf "$(GREEN)✓ LLM API key present"; \
	  [ -n "$$OPENAI_API_KEY" ]          && printf " (OpenAI)"; \
	  [ -n "$$ANTHROPIC_AUTH_TOKEN" ]    && printf " (Anthropic)"; \
	  [ -n "$$GEMINI_API_KEY" ]          && printf " (Gemini)"; \
	  printf "$(RESET)\n"; \
	fi

# ==============================================================================
# ── Build ──────────────────────────────────────────────────────────────────────
# ==============================================================================

.PHONY: build
build: ## Build the release binary
	@printf "$(BOLD)Building release binary...$(RESET)\n"
	cargo build --release --features cli
	@cp $(PDFIUM_LIB) target/release/libpdfium.dylib 2>/dev/null || true
	@printf "$(GREEN)✓ Built: $(RELEASE_BIN)$(RESET)\n"

.PHONY: build-dev
build-dev: ## Build the debug binary (faster, no optimisation)
	@printf "$(BOLD)Building debug binary...$(RESET)\n"
	cargo build --features cli
	@cp $(PDFIUM_LIB) target/debug/libpdfium.dylib 2>/dev/null || true
	@printf "$(GREEN)✓ Built: $(DEBUG_BIN)$(RESET)\n"

.PHONY: clean
clean: ## Remove build artifacts
	cargo clean
	@rm -f target/release/libpdfium.dylib target/debug/libpdfium.dylib
	@printf "$(GREEN)✓ Cleaned$(RESET)\n"

# ==============================================================================
# ── Inspect (no API needed) ────────────────────────────────────────────────────
# ==============================================================================

.PHONY: inspect-all
inspect-all: check-pdfium ## Inspect all test PDFs (page counts, metadata)
	@printf "$(BOLD)Inspecting test PDFs...$(RESET)\n"
	@for f in $(TEST_DIR)/*.pdf; do \
	  printf "\n$(CYAN)── $$f $(RESET)\n"; \
	  $(PDF2MD) --inspect-only "$$f" 2>&1; \
	done

.PHONY: inspect
inspect: ## Inspect a specific PDF: make inspect PDF=path/to/file.pdf
	@[ -n "$(PDF)" ] || (printf "$(RED)Usage: make inspect PDF=path/to/file.pdf$(RESET)\n"; exit 1)
	$(PDF2MD) --inspect-only "$(PDF)" 2>&1

# ==============================================================================
# ── Demo conversions (single pages — quick feedback) ───────────────────────────
# ==============================================================================

.PHONY: demo
demo: check-pdfium ## Convert page 1 of the Attention paper → stdout
	@printf "$(BOLD)Converting page 1 of Attention Is All You Need...$(RESET)\n"
	$(PDF2MD) --pages 1 "$(PDF_ARXIV)" 2>&1

.PHONY: demo-irs
demo-irs: check-pdfium ## Convert page 1 of the IRS form → stdout
	@printf "$(BOLD)Converting page 1 of IRS Form 1040...$(RESET)\n"
	$(PDF2MD) --pages 1 "$(PDF_IRS)" 2>&1

.PHONY: demo-neuro
demo-neuro: check-pdfium ## Convert page 1 of the neuroscience textbook → stdout
	@printf "$(BOLD)Converting page 1 of neuroscience textbook...$(RESET)\n"
	$(PDF2MD) --pages 1 "$(PDF_NEURO)" 2>&1

.PHONY: demo-url
demo-url: check-pdfium ## Convert page 1 of a live PDF from a URL
	@printf "$(BOLD)Converting page 1 from a URL...$(RESET)\n"
	$(PDF2MD) --pages 1 "https://arxiv.org/pdf/1706.03762" 2>&1

# ==============================================================================
# ── Full conversions (writes to test_cases/output/) ───────────────────────────
# ==============================================================================

.PHONY: convert-all
convert-all: check-pdfium $(OUT_DIR) ## Convert all test PDFs (pages 1-3 each)
	@printf "$(BOLD)Converting all test PDFs (pages 1-3)...$(RESET)\n"
	$(PDF2MD) --pages 1-3 --output $(OUT_DIR)/attention_is_all_you_need.md "$(PDF_ARXIV)" 2>&1
	$(PDF2MD) --pages 1-2 --output $(OUT_DIR)/irs_form_1040.md             "$(PDF_IRS)"   2>&1
	$(PDF2MD) --pages 1-3 --output $(OUT_DIR)/neuroscience_textbook.md     "$(PDF_NEURO)" 2>&1
	$(PDF2MD) --pages 1-2 --output $(OUT_DIR)/sample_text.md               "$(PDF_TEXT)"  2>&1
	@printf "$(GREEN)✓ Outputs written to $(OUT_DIR)/$(RESET)\n"

.PHONY: convert-paper
convert-paper: check-pdfium $(OUT_DIR) ## Convert full Attention paper (15 pages)
	@printf "$(BOLD)Converting Attention Is All You Need (all 15 pages)...$(RESET)\n"
	$(PDF2MD) --pages all --output $(OUT_DIR)/attention_full.md \
	  --separator hr --metadata "$(PDF_ARXIV)" 2>&1

.PHONY: convert-form
convert-form: check-pdfium $(OUT_DIR) ## Convert full IRS Form 1040
	@printf "$(BOLD)Converting IRS Form 1040...$(RESET)\n"
	$(PDF2MD) --pages all --output $(OUT_DIR)/irs_form_1040_full.md \
	  --separator hr "$(PDF_IRS)" 2>&1

$(OUT_DIR):
	@mkdir -p $(OUT_DIR)

# ==============================================================================
# ── Tests ──────────────────────────────────────────────────────────────────────
# ==============================================================================

.PHONY: test
test: ## Run unit tests (no API key needed)
	cargo test 2>&1

.PHONY: test-e2e
test-e2e: check-pdfium check-api-key build ## Run e2e integration tests
	@printf "$(BOLD)Running e2e tests...$(RESET)\n"
	DYLD_LIBRARY_PATH=$(ROOT_DIR) EDGEQUAKE_LLM_PROVIDER=openai EDGEQUAKE_MODEL=gpt-4.1-nano E2E_ENABLED=1 \
	  cargo test --test e2e -- --nocapture 2>&1

.PHONY: test-e2e-verbose
test-e2e-verbose: check-pdfium check-api-key build ## Run e2e tests with full output
	@printf "$(BOLD)Running e2e tests (verbose)...$(RESET)\n"
	DYLD_LIBRARY_PATH=$(ROOT_DIR) EDGEQUAKE_LLM_PROVIDER=openai EDGEQUAKE_MODEL=gpt-4.1-nano E2E_ENABLED=1 RUST_LOG=debug \
	  cargo test --test e2e -- --nocapture 2>&1

.PHONY: test-all
test-all: test test-e2e ## Run unit + e2e tests

# ==============================================================================
# ── Code Quality ───────────────────────────────────────────────────────────────
# ==============================================================================

.PHONY: lint
lint: ## Run clippy linter
	cargo clippy --all-features -- -D warnings 2>&1

.PHONY: fmt
fmt: ## Format source code with rustfmt
	cargo fmt 2>&1

.PHONY: fmt-check
fmt-check: ## Check formatting without modifying files
	cargo fmt --check 2>&1

.PHONY: doc
doc: ## Build and open documentation
	cargo doc --no-deps --open 2>&1

.PHONY: doc-test
doc-test: ## Test documentation examples
	cargo test --doc 2>&1

.PHONY: audit
audit: ## Check for security vulnerabilities in dependencies
	@command -v cargo-audit >/dev/null 2>&1 || (printf "$(YELLOW)Installing cargo-audit...$(RESET)\n" && cargo install cargo-audit)
	cargo audit 2>&1

.PHONY: ci
ci: fmt-check lint test doc-test ## Run all CI checks (format + lint + unit tests + docs)

.PHONY: ci-all
ci-all: fmt-check lint test doc-test audit build ## Run comprehensive CI checks (includes build + audit)

.PHONY: pre-publish
pre-publish: ## Run all pre-publish checks before release
	@bash scripts/pre-publish-check.sh

.PHONY: pre-publish-check-version
pre-publish-check-version: ## Run pre-publish checks with version verification
	@bash scripts/pre-publish-check.sh --version v$(shell grep '^version = ' Cargo.toml | head -1 | sed 's/version = "//' | sed 's/".*//')

.PHONY: msrv
msrv: ## Check minimum supported Rust version (1.80)
	@printf "$(BOLD)Checking MSRV (1.80)...$(RESET)\n"
	rustup toolchain install 1.80 --profile minimal 2>/dev/null || true
	cargo +1.80 check --all-features 2>&1
	@printf "$(GREEN)✓ MSRV check passed$(RESET)\n"

# ==============================================================================
# ── Utilities ──────────────────────────────────────────────────────────────────
# ==============================================================================

.PHONY: install
install: build ## Install pdf2md to ~/.cargo/bin
	cargo install --path . --features cli 2>&1
	@printf "$(GREEN)✓ pdf2md installed to ~/.cargo/bin$(RESET)\n"

.PHONY: bench-page
bench-page: check-pdfium ## Time conversion of a single page
	@printf "$(BOLD)Benchmarking single-page conversion...$(RESET)\n"
	time $(PDF2MD) --pages 1 "$(PDF_ARXIV)" >/dev/null 2>&1

.PHONY: view-output
view-output: ## Open the test output directory
	@ls $(OUT_DIR) 2>/dev/null && open $(OUT_DIR) || printf "$(YELLOW)No output yet — run: make convert-all$(RESET)\n"

.PHONY: download-test-pdfs
download-test-pdfs: ## Re-download all test PDFs
	@printf "$(BOLD)Downloading test PDFs...$(RESET)\n"
	@mkdir -p $(TEST_DIR)
	curl -fSL "https://arxiv.org/pdf/1706.03762"                                            -o $(TEST_DIR)/attention_is_all_you_need.pdf && printf "$(GREEN)✓ Attention paper$(RESET)\n"
	curl -fSL "https://www.irs.gov/pub/irs-pdf/f1040.pdf"                                   -o $(TEST_DIR)/irs_form_1040.pdf && printf "$(GREEN)✓ IRS Form 1040$(RESET)\n"
	curl -fSL "https://css4.pub/2015/textbook/somatosensory.pdf"                            -o $(TEST_DIR)/neuroscience_textbook.pdf && printf "$(GREEN)✓ Neuroscience textbook$(RESET)\n"
	curl -fSL "https://freetestdata.com/wp-content/uploads/2021/09/Free_Test_Data_1MB_PDF.pdf" -o $(TEST_DIR)/sample_text.pdf && printf "$(GREEN)✓ Sample text PDF$(RESET)\n"

# Informational targets (no-op but useful for discovery)
.PHONY: info
info: ## Show project info and current configuration
	@printf "$(BOLD)Project:$(RESET)     edgequake-pdf2md\n"
	@printf "$(BOLD)Root:$(RESET)        $(ROOT_DIR)\n"
	@printf "$(BOLD)Binary:$(RESET)      $(BIN)\n"
	@printf "$(BOLD)pdfium:$(RESET)      "
	@[ -f "$(PDFIUM_LIB)" ] && printf "$(GREEN)present$(RESET) $(shell ls -lh $(PDFIUM_LIB) | awk '{print $$5}')" || printf "$(RED)MISSING$(RESET)"
	@printf "\n$(BOLD)Test PDFs:$(RESET)   $(shell ls -1 $(TEST_DIR)/*.pdf 2>/dev/null | wc -l | tr -d ' ') files\n"
	@printf "$(BOLD)Provider:$(RESET)    $${EDGEQUAKE_PROVIDER:-auto-detect}\n"
	@printf "$(BOLD)Model:$(RESET)       $${EDGEQUAKE_MODEL:-provider default}\n"
