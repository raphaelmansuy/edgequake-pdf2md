# edgequake-pdf2md — Specification Index

> **Crate**: `edgequake-pdf2md`
> **Status**: Draft v0.1 — February 2026
> **Authors**: Engineering Team

---

## Document Map

| # | Document | Topic |
|---|----------|-------|
| 01 | [Overview & Goals](./01-overview.md) | Problem statement, objectives, design constraints |
| 02 | [Core Algorithm](./02-algorithm.md) | Pipeline stages, concurrency model, ASCII diagrams |
| 03 | [PDF Internals & Edge Cases](./03-pdf-internals.md) | PDF format deep-dive, failure modes, encoding quirks |
| 04 | [Markdown Output Spec](./04-markdown-spec.md) | Target Markdown format, element mapping, fidelity tiers |
| 05 | [Crate Selection](./05-crate-selection.md) | Library evaluation matrix, final choices, rationale |
| 06 | [Library API Design](./06-api-design.md) | Public Rust API, types, traits, builder patterns |
| 07 | [CLI Design](./07-cli-design.md) | Command-line interface spec, flags, UX principles |
| 08 | [Error Handling](./08-error-handling.md) | Error taxonomy, recovery strategies, user messaging |
| 09 | [Testing Strategy](./09-testing-strategy.md) | Unit, integration, golden-file, and LLM-mock tests |

---

## Cross-Reference Quick Guide

- **"How does the rendering pipeline work?"** → [02-algorithm.md §Pipeline](./02-algorithm.md#pipeline-stages)
- **"Which PDF crate should we use?"** → [05-crate-selection.md](./05-crate-selection.md)
- **"What does the public API look like?"** → [06-api-design.md](./06-api-design.md)
- **"How does the CLI work?"** → [07-cli-design.md](./07-cli-design.md)
- **"What can go wrong?"** → [08-error-handling.md](./08-error-handling.md) and [03-pdf-internals.md §Edge Cases](./03-pdf-internals.md#edge-cases)
- **"How do we integrate edgequake-llm?"** → [02-algorithm.md §LLM Integration](./02-algorithm.md#llm-integration)
- **"How is output Markdown structured?"** → [04-markdown-spec.md](./04-markdown-spec.md)

---

## External References

| Resource | URL |
|----------|-----|
| edgequake-llm docs | <https://docs.rs/edgequake-llm/latest/edgequake_llm/> |
| pdfium-render crate | <https://crates.io/crates/pdfium-render> |
| PDF specification (ISO 32000-2) | <https://pdfa.org/resource/pdf-specification-index/> |
| CommonMark spec | <https://spec.commonmark.org/> |
| Tokio async runtime | <https://tokio.rs/> |
| Clap CLI framework | <https://docs.rs/clap/latest/clap/> |
| Quantalogic PyZeroX (reference impl) | <https://github.com/quantalogic/quantalogic-pyzerox> |
| bblanchon/pdfium-binaries | <https://github.com/bblanchon/pdfium-binaries/releases> |

---

## Glossary

| Term | Definition |
|------|-----------|
| **VLM** | Vision-Language Model — a multimodal LLM that accepts both text and images |
| **Page bitmap** | A rasterised RGBA/RGB image of a single PDF page |
| **Fidelity tier** | Level of structural accuracy in the produced Markdown (T1=basic, T2=tables, T3=formulas) |
| **Provider** | An `edgequake-llm` LLM backend (OpenAI, Anthropic, Gemini, Ollama, …) |
| **Concurrency** | Maximum number of in-flight VLM API calls during batch processing |
| **Pdfium** | Google's C++ PDF engine, used by Chromium; wrapped by `pdfium-render` |
