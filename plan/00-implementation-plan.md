# Implementation Plan — edgequake-pdf2md

> Step-by-step actionable plan for implementing the Rust crate.
> Applies: First Principle Thinking, SRP, DRY.

---

## Phase 1: Scaffold

- [ ] **Step 1.1**: Create `Cargo.toml` per spec `05-crate-selection.md` §15
- [ ] **Step 1.2**: Create `src/lib.rs` as module root with re-exports

## Phase 2: Core Types (dependency-free, pure data)

- [ ] **Step 2.1**: `src/error.rs` — `Pdf2MdError` + `PageError` per spec `08-error-handling.md`
- [ ] **Step 2.2**: `src/config.rs` — `ConversionConfig`, builder, `FidelityTier`, `PageSelection`, `PageSeparator`
- [ ] **Step 2.3**: `src/output.rs` — `ConversionOutput`, `PageResult`, `ConversionStats`, `DocumentMetadata`
- [ ] **Step 2.4**: `src/prompts.rs` — default system prompt + maintain_format suffix (DRY: single source)

## Phase 3: Pipeline (SRP — one module per stage)

- [ ] **Step 3.1**: `src/pipeline/mod.rs` — module declarations
- [ ] **Step 3.2**: `src/pipeline/input.rs` — `resolve_input()`: detect URL vs file, download if needed
- [ ] **Step 3.3**: `src/pipeline/render.rs` — `render_pages()`: pdfium rasterisation → Vec<DynamicImage>
- [ ] **Step 3.4**: `src/pipeline/encode.rs` — `encode_page()`: DynamicImage → `ImageData` (base64)
- [ ] **Step 3.5**: `src/pipeline/llm.rs` — `process_page()`: build messages, call `provider.chat()`, extract content
- [ ] **Step 3.6**: `src/pipeline/postprocess.rs` — `clean_markdown()`: regex cleanup per spec `04-markdown-spec.md` §6

## Phase 4: Orchestration

- [ ] **Step 4.1**: `src/convert.rs` — `convert()`, `convert_to_file()`, `convert_sync()` 
- [ ] **Step 4.2**: `src/stream.rs` — `convert_stream()` streaming API
- [ ] **Step 4.3**: Update `src/lib.rs` with all public re-exports

## Phase 5: CLI Binary

- [ ] **Step 5.1**: `src/bin/pdf2md.rs` — clap derive argument struct + main()

## Phase 6: Tests & Verification

- [ ] **Step 6.1**: Unit tests for `postprocess`, `config`, `encode`
- [ ] **Step 6.2**: Integration test with `MockProvider`
- [ ] **Step 6.3**: `cargo build` (verify compilation)
- [ ] **Step 6.4**: `cargo clippy` and `cargo test`

---

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────┐
│                       convert()                          │
│  ┌─────────┐  ┌──────────┐  ┌────────┐  ┌───────────┐  │
│  │  input   │→│  render   │→│ encode │→│    llm     │  │
│  │ resolve  │  │ (pdfium)  │  │(base64)│  │(edgequake)│  │
│  └─────────┘  └──────────┘  └────────┘  └───────────┘  │
│                                              │           │
│                                    ┌─────────┴─────┐    │
│                                    │ postprocess    │    │
│                                    │ (regex clean)  │    │
│                                    └───────────────┘    │
└─────────────────────────────────────────────────────────┘
```

## Key Design Decisions

1. **Provider injection**: `config.provider` is `Option<Arc<dyn LLMProvider>>`. If `None`, auto-detect via `ProviderFactory`.
2. **Concurrency**: `futures::stream::iter(pages).buffer_unordered(config.concurrency)`
3. **maintain_format**: Sequential processing — each page receives prior page's markdown as context.
4. **Error granularity**: Page failures → `PageResult` with `error: Option<PageError>`, not abort.
5. **Pdfium binding**: `Pdfium::default()` (tries local dir, then system library).
