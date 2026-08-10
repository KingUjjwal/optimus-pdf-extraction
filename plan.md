# Optimus — Improvement Plan

> **Last audited: 2026-08-08** — current source verified against reality (previous plan.md was stale).
> **Source of ideas:** `firecrawl/pdf-inspector` (MIT, 13.2k★) — `src/detector.rs`, `src/text_quality.rs`, `src/tables/mod.rs`, `src/markdown/analysis.rs`, plus two parallel auto-research agents weighing the extractor-engine decision (Phase 6 / Appendix A).
> **Primary constraint:** fastest runtime performance. **Secondary constraint:** keep compile times sane (see Compile-Time Safeguards).

---

## Progress Tracker (2026-08-08)

| Item | Status |
|---|---|
| CS-1 profile fix + JIT wasm determinism | ✅ `5b1e4b3` |
| 1.1 text-quality gating | ✅ `5b1e4b3` |
| 1.2 PDF classification (lopdf + span fallback) | ✅ `369a24b` |
| 1.3 scanned-PDF OCR routing | ✅ `5bf83e8` |
| 6.1 pdf_oxide 0.2→0.3.77 + font metadata | ✅ `aa84aa1` |
| 2.4 font-size rarity (`font_stats.rs`, grid footer, prose filter) | ✅ `78c4b1c` |
| 3.2 stale codegen prompt fix | ✅ `78c4b1c` |
| 3.1 token-efficient `compact_grid` | ✅ `e5b55cf` |
| 2.1 key-value detector (`table_kv.rs`) | ✅ committed |
| 2.3 layout priors in codegen | ✅ `a29ea69` |
| 2.6 TOC entry guard | ✅ `a7850ce` |
| 7.1 README/AGENTS.md sync | ✅ `8fb5092` |
| 2.5 per-field extraction confidence | ✅ `ab125e1` |
| 5.3 confidence badge in results view | ✅ `ef19fc7` |
| 2.2 columnar-table detector (`table_columns.rs`) | ✅ committed |
| 4.x `optimus-eval` crate + `make eval` | ✅ committed |

**Remaining backlog:** 5.1 per-page quality map in wizard · 5.2 classification badge on upload (partially in 1.3) · 5.4 OCR-routing indicator · 5.5 extra event plumbing · 2.2 rect-guided detector refinement.

---

## Phase 0 — State Audit (verified against source, not theory)

| Area | Previous docs said | Reality (2026-08-08) |
|---|---|---|
| Phase 7 wizard | PLANNED | **DONE** — `ingest_command`, `infer_schema_llm_command`, `compile_module_llm_command`, `extract_cached_command`, `save_llm_config_command`, `batch_extract_command`, `get_llm_history_command` + `Wizard.tsx`, `Step0_Upload.tsx` |
| Tauri commands | 8 / 12 events | **15 commands** (`src-tauri/src/commands/` ×9 modules: batch, cache, compile, extract, ingest, llm, mod, schema, types), **18 events** (`src/lib/events.ts` incl. `classify-done`, `ocr-needed`) |
| Guest stdlib | 5 fns | **15+ fns** — incl. `find_transaction_rows`, `rows_below`, `cell_for_header`, `row_to_object`, `build_json_typed`/`emit_json_typed`/`emit_json_typed_with_confidence`, `find_label_value`, 9-field flat graph with coords |
| Agent | basic `infer_schema` | + `detect_transactions_columns`, `detect_key_value_pairs` (`table_kv.rs`), `detect_table_columns` (`table_columns.rs`), layout priors, `discover_schema_llm`, `record_llm_call`/`LlmCallHistory` |
| `TextSpan` | bbox only | + `page`, `font_size`, `is_bold`, `is_italic` (pdf_oxide 0.3) |
| LLM codegen prompt | 5-field graph | **9-field graph** documented + guest helper list + confidence rule (fixed in 3.2) |
| Fixtures | `invoice/report/form.pdf` | scored corpus with `*.ground_truth.json` + `optimus-eval` (field-F1 1.000 baseline) |
| `.cargo/config.toml` | — | cargo defaults (fast dev/test); WASM determinism pinned at the JIT call site |
| `optimus-guest` | — | **dep-free** (verified) — JIT temp crate pulls only `optimus-guest`; no extractor can leak into the wasm graph (fix in CS-1) |

---

## Decision D1 — Extractor Engine (RESOLVED by parallel auto-research)

**KEEP `pdf_oxide`, bump `0.2 → 0.3.x`. Add `lopdf 0.42` ONLY for classification. Do NOT swap the extractor to `pdf-inspector`.**

### Rationale (see Appendix A for full research + sources)
1. **Fastest runtime.** `pdf_oxide` claims 0.8 ms mean / 9 ms p99 raw span extraction and 100% pass on a 3,830-PDF corpus. `pdf-inspector`'s 2.35 ms/doc is the *full* detect+extract+reading-order+tables+markdown pipeline — not apples-to-apples, but both are sub-ms for the span extraction Optimus needs. Keeping `pdf_oxide` = zero migration risk, zero new dependency graph, same call sites (`PdfDocument::open` / `extract_spans`).
2. **Compile time.** The swap would be roughly neutral (pdf-inspector resolves to ~131 crates vs current 175), so it's *not* a compile-time win either — and it adds churn. The bump is incremental and cheap.
3. **Security.** Published `pdf-inspector 0.1.7` pins `lopdf ^0.41.0` → **RUSTSEC-2026-0187** (stack-overflow DoS, SIGABRT on ~21 KB crafted PDF). Only `main` bumps to lopdf 0.42. Git-pinning a pre-1.0 crate is a maintenance burden Optimus doesn't need.
4. **Correctness.** Raw `lopdf` DIY extraction = ~80% corpus pass rate (garbled-CMap risk). `pdf_oxide` 0.3 carries 80 releases of CMap/CID/reading-order fixes.
5. **The value is portable.** Everything worth stealing from pdf-inspector (quality gates, classification, table detectors) is a **self-contained heuristic** that ports over `pdf_oxide` spans — no engine swap required.

### Fallback trigger
If, after the 0.3 bump, the fixtures regress or font metadata is unusable, reconsider `pdf-inspector` **from `main` (lopdf 0.42)** as a second extraction path behind a feature flag — not as the default.

---

## Phase 1 — Pipeline Robustness (P0)

### 1.1 Text-Quality Gating — stop feeding garbage to the LLM
- **Goal:** reject mojibake/garbled spans before R-Tree + schema inference. Today: zero quality validation; a broken ToUnicode CMap → LLM hallucinates schema on garbage and burns `CostTracker` budget.
- **Port** `pdf-inspector/src/text_quality.rs` (self-contained, no deps):
  - `is_garbage_text` — alphanumeric-vs-non-alphanumeric ratio with decorative-leader run handling
  - `detect_encoding_issues` — U+FFFD runs, dollar-as-space, **substitution-cipher cosine detector** (English freq cosine < 0.60 ∧ shape-cosine ≥ 0.90)
  - `is_cid_garbage` — C1-control (U+0080–9F) + high-Latin mojibake ratios
  - `analyze_text_quality` — per-page evidence accumulation (one garbage span must not condemn a page)
- **Files:** new `optimus-core/src/text_quality.rs`; hook into `extract_spans` (filter + flag); gate `infer_schema` / `discover_schema_llm` in `optimus-agent/src/schema.rs`; emit `has_encoding_issues` on `pipeline:ingest-done`.
- **Tests:** port pdf-inspector unit fixtures (`8VceZWZTReV`, dollar-pattern, U+FFFD runs).
- **Effort:** 1–2 d · **Risk:** low.

### 1.2 PDF Classification → OCR Routing
- **Goal:** detect `TextBased/Scanned/ImageBased/Mixed` in ~10–50 ms *before* the LLM compile loop. Today scans hit `Err(NoSpansFound)`.
- **Port** `pdf-inspector/src/detector.rs` logic:
  - content-stream sampling for `Tj`/`TJ` vs `Do` operators; `ScanStrategy { EarlyExit, Full, Sample(n), Pages(vec) }`
  - image-dominance ratio, **vector-text** (`path_ops ≥ 1000 ∧ path_ops > text_ops·200`), **Identity-H-without-ToUnicode**, **Type3-only**, **newspaper** (Tf/Tj ratio)
  - confidence + per-page `pages_needing_ocr` + machine-readable reasons
- **Engine:** add `lopdf = "0.42"` (RUSTSEC-safe) to `optimus-core` (or `optimus-router`) **for classification only** — small (~200 KB), wasm32-compatible, never linked into the JIT graph.
- **Files:** new `optimus-core/src/classify.rs`; pre-stage before `calculate_layout_id`; extend ingest payload with `pdf_type` + `pages_needing_ocr`.
- **Tests:** scanned + text fixture pair; vector-text and Identity-H synthetic PDFs.
- **Effort:** 2–3 d · **Risk:** med (`lopdf` co-exists with `pdf_oxide`; verify under new profile settings).

### 1.3 Graceful Scanned-PDF Handling
- **Goal:** replace the `#[cfg(test)]` mock fallback semantics with real routing. On `Scanned`/`ImageBased`: emit `pipeline:ocr-needed`, skip compile, show "OCR required" in the wizard instead of failing.
- **Files:** `optimus-core/src/lib.rs` error path; `optimus-agent/src/compiler.rs` early-return; `Wizard.tsx`.
- **Effort:** 0.5 d · **Risk:** low.

---

## Phase 2 — Deterministic Extraction Intelligence (P1)

### 2.1 Key-Value Table Detector
- **Goal:** deterministic `Field: Value` extraction without the LLM — this IS Optimus's invoice/statement use-case and becomes the offline fallback.
- **Port** `pdf-inspector/src/tables/mod.rs` `try_build_key_value_table_from_rows`: X-gap split inference (≥2× font-size median gap), visual-row grouping, header inference, prose-veto guards, section rows, EDGAR-tag special-case.
- **Files:** new `optimus-agent/src/table_kv.rs` over `TextSpan`; wire as `infer_schema` fallback + candidate cells for the LLM prompt (2.3).
- **Tests:** port + fixture assertions.
- **Effort:** 2 d · **Risk:** low–med.

### 2.2 Column / Rect-Guided Table Detectors
- **Port** `try_build_table_from_columns` (borderless: fill ≥15%, ≥50% multi-col rows, cell ≤40 chars, prose-cell ≤15%) and `try_build_rect_guided_table` (rect X → column boundaries, 2 pt snap, median-gap interpolation, `split_merged_numbers` for `"10 11 12 13"` tokens).
- **Files:** `optimus-agent/src/table_columns.rs`; extend existing `detect_transactions_columns` in `schema.rs`.
- **Effort:** 2 d · **Risk:** med (prose false-positives — the guards matter).

### 2.3 Table Priors into LLM Codegen
- **Goal:** stop the LLM from discovering geometry blind. Pass precomputed `header → [rows]` candidates into `code_generation_user` (`templates.rs`); generated guest reads the prior instead of blind `find_right_of`.
- **Payoff:** fewer extraction-fix loops → directly cuts LLM cost; better first-attempt compile.
- **Effort:** 1 d · **Risk:** low.

### 2.4 Font-Size Rarity for Anchor / Schema Inference
- **Port** `markdown/analysis.rs` `font_size_rarity` + `compute_heading_tiers` (0.5 pt clustering, 4-tier cap, **exclude digit-only lines**, bold fallback below 1.2×).
- **Blocker → unblocked by Phase 6.1:** `pdf_oxide 0.3` spans carry font metadata. Then: weight `AnchorDetector` / `infer_schema` labels by rarity; add font-size column to the ASCII grid sent to the LLM.
- **Effort:** 1 d (post 6.1) · **Risk:** med.

### 2.5 Per-Field Extraction Confidence
- **Goal:** tag each extracted field `exact-label` / `heuristic-neighbor` / `fuzzy`; surface in `ArrowTableView`.
- **Files:** `optimus-guest` `JsonValue`/`emit_json_typed` (add confidence), `optimus-runtime` record builder.
- **Note:** guest format change → recompile cached WASM → **bump `CACHE_VERSION`**.
- **Effort:** 1–2 d · **Risk:** low–med.

### 2.6 Table-of-Contents Detection
- **Port** `is_toc_entry_line` / `is_toc_marker_heading` + `TableKind::Toc` classification so TOCs are never extracted as transaction data.
- **Files:** `optimus-agent/src/schema.rs` guard in `detect_transactions_columns`.
- **Effort:** 0.5 d · **Risk:** low.

---

## Phase 3 — LLM Cost & Prompt Hygiene (P1)

### 3.1 Token-Efficient Grid
- **Port** `pdf2md --compact` idea: collapse dot-leader runs + long whitespace in the ASCII grid before LLM (schema + codegen prompts). Direct `CostTracker` saving.
- **Files:** `optimus-core` grid generation + `compact_grid()` helper.
- **Effort:** 0.5–1 d · **Risk:** low (additive, off by default).

### 3.2 Fix Stale Codegen Prompt (bug today)
- `CODE_GENERATION_SYSTEM` documents the **5-field** flat-graph format; guest emits **9-field** now. Update and document the transaction helpers (`find_transaction_rows`, `cell_for_header`, `row_to_object`, `find_label_value`) in the rules list.
- **Files:** `optimus-agent/src/templates.rs`.
- **Effort:** 0.25 d · **Risk:** low.

---

## Phase 4 — Quality Measurement (P1) · new `optimus-eval` crate

> **Compile-time note:** `optimus-eval` uses ONLY existing workspace crates + `serde_json`. No heavy deps. Verify with `cargo check -p optimus-eval` after adding.

### 4.1 Benchmark Corpus + Ground Truth
- Extend `optimus-core/tests/fixtures/` with a small scored corpus (invoices, statements, reports); hand-authored `ground_truth.json` (fields + transactions) per doc.
- **Effort:** 1–2 d.

### 4.2 Metrics
- Port opendataloader-bench scoring (reading-order NID, table TEDS, heading MHS) adapted to structured output. **v1:** field-level F1 + transaction-row exact-match rate + runtime ms/doc.
- **Files:** new `optimus-eval/src/lib.rs` + `src/bin/optimuseval.rs`.
- **Effort:** 2–3 d.

### 4.3 CLI + Make + CI Wiring
- `optimus-cli` `eval` subcommand (or call the bin); `make eval`; run as **warn-only** in `make test-all`.
- **Effort:** 0.25 d.

### 4.4 Baseline
- Capture the pre-change baseline (current fixtures) so Phases 1–2 are verifiable regressions, not vibes.

---

## Phase 5 — UI/UX & Observability (P2)

| # | Item | Detail | Effort |
|---|---|---|---|
| 5.1 | Per-page quality map | `pages_needing_ocr` + reasons rendered in `Wizard.tsx` step 4 | 0.5–1 d |
| 5.2 | Classification badge | `TextBased/Scanned/Mixed` chip on `Step0_Upload.tsx` after ingest | 0.5 d |
| 5.3 | Confidence in results | per-field confidence in `ArrowTableView.tsx` (needs 2.5) | 0.5 d |
| 5.4 | OCR-routing indicator | `pipeline:ocr-needed` event → banner + "skip compile" affordance | 0.5 d |
| 5.5 | New event plumbing | `classify-done`, `ocr-needed`, `quality-warning` in `events.ts` + backend | 0.5 d |

---

## Phase 6 — Extractor Engine Upgrade (Decision D1 implementation)

### 6.1 Bump `pdf_oxide 0.2 → 0.3.x`
- **Goal:** font metadata (font size/name, bold/italic, width) + 80 releases of CMap/CID/reading-order fixes + (claimed) 100% corpus pass.
- **Files:** `optimus-core/Cargo.toml`; adapt `extract_spans` accessors; extend `TextSpan` with `font_size`, `is_bold`, `is_italic`, `width` (serde-compatible — new fields optional).
- **Risks:** 0.3.x is fast-moving (near-daily releases) — pin an exact version; MSRV 1.88 (check toolchain). API accessor drift possible.
- **Tests:** run all fixtures; compare span counts + glyph quality vs 0.2 baseline.
- **Effort:** 1–2 d.

### 6.2 Add `lopdf 0.42` (classification only)
- New `optimus-core/src/classify.rs` (Phase 1.2). Keep it out of `optimus-guest`/`optimus-router`/`optimus-agent`/`optimus-runtime`/`src-tauri` dependency trees. Use `default-features = false` if `rayon`/`chrono` are unneeded for classification.
- **Effort:** 0.5 d.

### 6.3 Optional: reading-order pipeline
- `pdf_oxide 0.3` ships a `TextPipeline` with pluggable reading order (XY-Cut / Structure Tree / Geometric). If 0.3 exposes ordered spans, use them for the ASCII grid + flat graph (better multi-column handling than the current raw bbox ordering).
- **Effort:** 1 d (evaluate during 6.1).

---

## Phase 7 — Documentation & Hygiene (P2)

### 7.1 Sync stale docs
- `README.md` / `AGENTS.md`: correct command counts (15 / 16 events), mark Phase 7 DONE, update file map (`commands/` dir, `Wizard.tsx`, `Step0_Upload.tsx`, `optimus-eval`).
- **Effort:** 0.5–1 d.

### 7.2 Conventions for new modules
- `#[tracing::instrument]` on all new public fns (repo convention); AGENTS.md gotchas section for text-quality / classification / eval.
- **Effort:** included per-phase.

---

## Compile-Time Safeguards (CS)

### CS-1 — Fix `.cargo/config.toml` profile overhead
- **Remove `incremental = false, codegen-units = 1` from `[profile.dev]` and `[profile.test]`.** Verified: the JIT build (`compiler.rs:167`) runs in a standalone temp crate with `--release` and its own `[workspace]` — these settings neither speed the wasm build nor make it deterministic; they only slow every dev `cargo check`/`cargo test` over a 175-crate graph.
- **Move wasm determinism to the JIT call site:** in `compiler.rs`, set `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1` (and optionally `CARGO_PROFILE_RELEASE_LTO=true`) as env vars on the `Command`, or write `[profile.release] codegen-units = 1` into the generated temp `Cargo.toml`. Launch-dir-independent and explicit.
- **Effort:** 0.5 d · **Risk:** low. Measure `cargo check` before/after.

### CS-2 — Dependency discipline
- Extractor deps (`pdf_oxide`, `lopdf`) live in `optimus-core` only. `optimus-guest` stays dep-free (verified) → the runtime JIT graph can never grow. New `optimus-eval` adds only `serde_json`.
- Before merging any phase: `cargo check --workspace` + `cargo test --workspace --lib` and record wall time.

### CS-3 — CI budget
- Target: workspace `cargo test` ≤ ~5 min warm (upstream pdf-inspector/lopdf CI are ~2–3 min each on warm runners — HIGH confidence, Appendix A).

---

## Execution Order & Priorities

| Order | Item | Effort | Payoff |
|---|---|---|---|
| 1 | CS-1 profile fix | 0.5 d | Dev-loop speed immediately |
| 2 | 1.1 Text-quality gating | 1–2 d | Protects LLM spend + correctness |
| 3 | 1.2 + 1.3 Classification/routing | 2–3 d | Unblocks scans, sane errors, batch routing |
| 4 | 6.1 pdf_oxide bump (font metadata) | 1–2 d | Unlocks 2.4; CMap fixes |
| 5 | 3.2 + 3.1 prompt/grid hygiene | 1 d | Cost + bug fix |
| 6 | 2.1 Key-value detector | 2 d | Offline extraction + LLM prior |
| 7 | 4.1 + 4.2 Benchmark harness | 3–4 d | Baseline for everything after |
| 8 | 2.2, 2.3, 2.5, 2.6 | 4–5 d | Extraction quality |
| 9 | Phase 5 UI | 2–3 d | User-visible value |
| 10 | 7.1 doc sync | 1 d | Hygiene |

**Total: ~19–26 days.** P0 (correctness/cost) = items 1–5; P1 (extraction quality) = items 6–8; P2 (polish) = items 9–10.

---

## Risk Register

| Risk | Mitigation |
|---|---|
| `pdf_oxide 0.3.x` API drift (fast-moving) | Pin exact version; accessor shim in `optimus-core`; fixture regression gate |
| `pdf_oxide` MSRV 1.88 vs toolchain | Check `rustc --version`; bump pinned toolchain in Makefile if needed |
| Classification false positives (prose→table, scanned→text) | Port the guard thresholds verbatim; add fixture tests before tuning |
| Text-quality gate false positives (math formulas → OCR) | Per-page evidence accumulation + density thresholds (port exactly) |
| Guest format change (2.5) breaks cached WASM | Bump `CACHE_VERSION`; `LayoutHealth` flags stale v2 entries |
| `optimus-eval` compile bloat | Dep-light by rule (CS-2); `cargo check -p optimus-eval` gate |
| PDFs are untrusted input | lopdf 0.42 (RUSTSEC-2026-0187 fixed); reject pdf-inspector 0.1.7 published pin |

---

## Appendix A — Extractor Decision Research (auto-research agents, 2026-08-08)

### A.1 Option comparison (Agent 1: runtime/quality)

| Option | Runtime evidence | Font metadata | CID/CMap | Maintenance | wasm32 | Risk |
|---|---|---|---|---|---|---|
| **pdf_oxide** (keep, bump 0.3) | 0.8 ms mean / 9 ms p99; 5× PyMuPDF, 15× pypdf; 100% pass 3,830-PDF corpus (self-pub) | Yes — span-level font info + `TextPipeline` reading order | encoding_rs CMaps + ttf-parser; Strong | Very active (82 releases, 512k dl, single maintainer) | Yes (wasm feature) | Large tree (~6.6 MB, MSRV 1.88), 0.3.x breakage |
| **pdf-inspector** (replacement) | 2.35 ms/doc full pipeline (0.470 s/200 docs) | Best-in-class `TextItem` (font, size, bold/italic/underline) | ToUnicode CMap parser, Type0/Identity-H | Active but **pre-1.0** (0.1.7, Jul 31 2026), 2 months old | Yes (dedicated wasm/) | API churn; **RUSTSEC-2026-0187 via lopdf ^0.41** on published crate |
| **lopdf** (direct) | 0.3 ms raw (hand-rolled) | None — DIY | **DIY = 80.2% pass rate** (garbled-text risk) | Active (0.44, 14.5M dl) | Yes (wasm_js) | Highest dev cost + correctness risk |

### A.2 Compile-time findings (Agent 2)

- Current `optimus-core` native tree = **175 crates** (pdf_oxide pulls image, regex, nom, flate2, chrono, phf, ttf-parser…). `pdf-inspector` 0.1.7 resolves to **131** native / 114 wasm — the swap would be *smaller*, but **not** a decisive win, and adds rayon/jiff/time/encoding_rs.
- `lopdf` 0.42 is small (~204 KB); `default-features = false` slims it further (~19 deps).
- **RUSTSEC-2026-0187** (HIGH confidence): lopdf ≤0.41 unbounded-recursion stack overflow, SIGABRT, uncatchable. Fixed in 0.42. pdf-inspector 0.1.7 published pins `^0.41`; `main` bumps to 0.42. A `[patch.crates-io]` cannot override `^0.41` (semver mismatch) — only a git pin or vendor escapes.
- JIT isolation **verified in repo**: `compiler.rs:146-162` writes a temp crate depending only on `optimus-guest` (dep-free, verified). No extractor ever enters the runtime wasm build.
- `.cargo/config.toml` dev/test `incremental=false, codegen-units=1` **does not govern the JIT** (standalone temp crate, launch-dir-dependent `cache_dir`); it only slows the workspace dev loop.
- Upstream CI reference (HIGH confidence): pdf-inspector ~2–2.6 min, lopdf ~3 min warm.

### A.3 Decision rationale
- Fastest runtime: pdf_oxide keeps the fastest *span* path with API continuity; pdf-inspector's headline figure is a full-pipeline number.
- Compile time: swap is neutral, not a win; bump is cheap.
- Security: published pdf-inspector carries a known DoS pin; git-pinning pre-1.0 is a maintenance liability.
- Correctness: raw lopdf DIY = 80% pass; pdf_oxide 0.3 has the fixes.
- All pdf-inspector *value* (quality gates, classification, tables) is portable self-contained heuristics — engine swap unnecessary.

### A.4 Sources
- crates.io API: `pdf_oxide`, `pdf-inspector`, `lopdf`, `pdf-extract`, `ttf-parser` (versions, downloads, dep tables)
- github.com/yfedoseev/pdf_oxide (README bench, commits) · docs.rs/pdf_oxide/0.3.77 (spans, TextPipeline, MSRV, deps)
- github.com/firecrawl/pdf-inspector (README bench, `main` Cargo.toml lopdf 0.42) · docs/rust-api.md · wasm/README.md · src/types.rs
- deps.rs: `lopdf/0.42.0`, `pdf-inspector/0.1.7` (dep tables + **RUSTSEC-2026-0187**)
- GitHub Actions runs API: pdf-inspector CI (2–2.6 min), lopdf CI (~3 min)
- Local repo probes (HIGH): `cargo tree` (175 / 131 / 114), `.cargo/config.toml`, `optimus-guest/Cargo.toml`, `optimus-agent/src/compiler.rs:146-175`
