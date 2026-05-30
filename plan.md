# Optimus — Implementation Plan

> **Last audited: 2026-05-26** — full source verification against reality.
> All 12 tracked bugs FIXED. All 8 remaining items (R1-R8) DONE.
> Schema inference, cache management, output export all implemented.
> Next: Multi-step extraction wizard (Phase 7).

## Current State

| Component | Phase | Completion | Status |
|-----------|-------|-----------|--------|
| `optimus-core` | 1 — Ingestion & Spatial Mapping | 98% | All core types, extraction, graph, grid, benchmarks done. Mock fallback gated behind `#[cfg(test)]`. |
| `optimus-router` | 2 — Layout Fingerprinting | 98% | `LayoutDb`, `AnchorDetector`, regex patterns, multi-page anchors, cache repair, `LayoutHealth` validation. |
| `optimus-agent` | 3 — Agentic Compiler Loop | 98% | Full compile loop, OpenAI + Anthropic providers, cost tracking, offline mode. Heuristic `infer_schema` with spatial right-neighbor type detection. Cache versioning (`CACHE_VERSION=2`). `TempArtifactGuard` for cleanup on panic. |
| `optimus-runtime` | 4 — High-Throughput Execution | 95% | `WasmHost` shared via `Arc` across Rayon threads. Dynamic `ExtractedRecord`. Streaming pipeline. Dynamic Arrow batch builder. |
| `optimus-guest` | 4 — Guest Stdlib | 100% | `parse_flat_graph`, `find_right_of`, `find_below`, `emit_json`, `alloc`/`free_buf` all done. |
| `optimus-cli` | 5 — CLI | 98% | 9 subcommands. `--format json|arrow` works. `tracing-subscriber` initialized. Ctrl+C graceful shutdown. |
| `src-tauri` | 6 — Tauri Desktop App | 95% | 8 commands (7 async, 3 sync). Per-iteration compile progress events. `tracing-subscriber` with env filter. `delete_cache_entry_command`. `#[tracing::instrument]` on all commands. |
| `src/` (frontend) | 6 — SolidJS UI | 95% | 7 components. Tab-based layout. Event system with 12 listeners. Error banner + processing indicator. Interactive cache browser with detail panel, per-entry delete, version badge. ArrowTableView with Copy JSON/CSV. Schema inference with heuristic spatial detection. |

---

## Bug Tracker — All Resolved

| # | Severity | Component | Issue | Status |
|---|----------|-----------|-------|--------|
| G1 | Critical | `optimus-runtime` | Each Rayon thread creates its own `WasmHost` | **FIXED** — single `Arc<WasmHost>` shared |
| G2 | Critical | `optimus-agent` | `optimus-guest` not imported by generated code | **FIXED** — `use optimus_guest::*` in template |
| G3 | Critical | Frontend | `SpatialGraphView` never receives span data | **FIXED** — set from `ingest-done` event + `processPdf` |
| G4 | High | Frontend | `SchemaEditor` not rendered | **FIXED** — rendered in Schema tab |
| G5 | High | `optimus-runtime` | `ExtractedRecord` hardcoded to invoice fields | **FIXED** — `HashMap<String, String>` with `#[serde(flatten)]` |
| G6 | Medium | `optimus-runtime` | `WasmResult` struct defined but unused | **FIXED** — removed entirely |
| G7 | Medium | `optimus-router` | `is_layout_cached` opens fresh sled per call | **FIXED** — CLI uses `is_layout_cached_with_db` |
| G8 | Medium | `optimus-core` | `extract_spans` falls back to mock spans silently | **FIXED** — `#[cfg(test)]` gate + `log::warn!` |
| G9 | Medium | Frontend | `cacheDir` relative path | **FIXED** — uses `appDataDir()` + `join()` |
| G10 | Low | `optimus-guest` | `build_json` uses `format!` (not `no_std`) | **FIXED** — manual `push`/`push_str` |
| G11 | Low | CLI | `--format` flag ignored | **FIXED** — Arrow IPC output implemented |
| G12 | Low | Tauri | `GraphResult`/`SchemaFieldDef` dead types | **FIXED** — removed entirely |

---

## Remaining Work — All Done

### R1 — Async Tauri Commands — DONE
All Tauri commands converted to `async fn` with `tokio::task::spawn_blocking`. UI no longer freezes during compile loops.

### R2 — Compile Progress Streaming — DONE
`pipeline:compile-attempt` event with `{ layout_id, attempt, status, errors? }` per compile iteration.

### R3 — Frontend Error Handling — DONE
Error banner with click-to-dismiss, animated processing indicator, error cleared on next `processPdf` call.

### R4 — `tracing-subscriber` in Tauri — DONE
Initialized in `src-tauri/src/main.rs` with `env_filter` + `with_target(false)`.

### R5 — CLI Sled Reopen Fix — DONE
`extract_single` opens `LayoutDb` once, uses `is_layout_cached_with_db`.

### R6 — Stale Cache Versioning — DONE
`LayoutManifest` includes `cache_version` field (`CACHE_VERSION=2`). Frontend shows v1/v2 badge.

### R7 — Schema Inference — DONE
`infer_schema(spans)` uses spatial right-neighbor heuristic: finds `Label:` spans → nearest right value → infers type (date/number/string). LLM path exists in `discover_schema` but not yet wired from Tauri.

### R8 — Error Recovery & Resilience — DONE
`TempArtifactGuard` (Drop-based temp file cleanup on panic). `LayoutHealth` + `validate_layout()` / `repair_layout()` for corrupted cache detection. Ctrl+C graceful shutdown in CLI `watch` mode.

---

## Additional Work Completed

### Schema Inference System
- **`optimus-agent/src/schema.rs`**: `infer_schema(spans)` — heuristic spatial field detection.
  - Finds spans ending with `:` → labels
  - Searches for nearest span to the right (10px vertical tolerance, center-point comparison)
  - Infers field type: `date` (ISO YYYY-MM-DD, US MM/DD/YYYY), `number` (currency, digits, percentages), `string`
  - Deduplicates field names, returns JSON schema
  - Falls back to hardcoded `{"invoice_number":"string","date":"string","total":"string"}` if no fields detected
  - Unit tested: `test_infer_schema`, `test_infer_field_type`
- **Tauri command** `discover_schema_command` now calls `infer_schema(&spans)` instead of hardcoded fallback.
- LLM-based `discover_schema` async function exists (OpenAI/Anthropic) but not yet exposed as Tauri command.

### Cache Management
- **`delete_cache_entry_command`**: Per-entry delete (sled + filesystem). Registered in Tauri.
- **`CacheBrowser.tsx`**: Rewritten with:
  - Click-to-expand detail panel (full `layout_id`, `schema`, `model`, `compile_attempts`, `cache_version`, `created_at`)
  - Cache version badge: green `v2` (current), red `v1` (stale with warning)
  - Per-entry "Delete" button
  - Expand/collapse arrow indicator
- **`commands.ts`**: Added `deleteCacheEntry(layoutId, cacheDir)` frontend binding.

### Output Export
- **`ArrowTableView.tsx`**: Added "Copy JSON" (formatted with `JSON.stringify(record, null, 2)`) and "Copy CSV" (header row + value row with proper quoting) buttons via `navigator.clipboard.writeText`.

### Observability
- **`#[tracing::instrument]`** added to all public functions across `optimus-core`, `optimus-router`, `optimus-agent`, `optimus-runtime`, `optimus-cli`, and `src-tauri`.
- `tracing` dependency added to core, router, agent.
- Structured fields: `path`, `layout_id`, `span_count`, `node_count`, `input`, `output`, `count`, `dir`.

### Frontend Polish
- **`PdfDropZone.tsx`**: Split into two-step (select → Extract). Added "Clear" button. Processing state disables buttons to prevent double-submit.
- **`App.tsx`**: `onInfer` now calls `discoverSchema(spansJson)` with current spans instead of broken `extractDocument("")`. Shows warning if no spans loaded.
- **`.jsx`/`.js` duplicates**: Deleted 12 duplicate files — only `.tsx`/`.ts` remain.

### Build System
- **`Makefile`**: 60+ targets across 10 categories (build, test, dev, deploy, graphify, CLI, bench, clean, stats, audit).
- Auto-detects cargo path (`USERPROFILE/.cargo/bin` on Windows).
- LLM-aware `install-rust`, `bundle`, `release`, `check-all`, `fmt-check`, `graphify-full`/`graphify-incremental`.

---

## Phase 1-6: All Complete

### Phase 1: optimus-core
| Item | Status |
|------|--------|
| `ExtractionError` enum with 5 variants + `Display`/`Error`/`From<io::Error>` | DONE |
| `extract_spans_from_bytes` with `TempDir` cleanup | DONE |
| Mock fallback gated behind `#[cfg(test)]` + `log::warn!` | DONE |
| `GridConfig { x_bucket, y_bucket }` with Default (8, 15) | DONE |
| `GridFormat` enum: `Ascii | MarkdownTable` | DONE |
| Cell collision handling in grid generation | DONE |
| Criterion benchmark harness | DONE |
| 4 integration tests (invoice, report, form, rtree perf) | DONE |

### Phase 2: optimus-router
| Item | Status |
|------|--------|
| `compute_anchor_distances` with `MAX_ANCHORS = 5` | DONE |
| `LayoutDb` (sled) with `open`, `lookup`, `store`, `list_layouts`, `remove`, `validate_layout`, `repair_layout` | DONE |
| `LayoutHealth` struct for cache corruption detection | DONE |
| `is_layout_cached_with_db` (avoids sled reopen) | DONE |
| Layout ID stability tests | DONE |
| `extract_anchors_multi_page`, `extract_anchors_first_page` | DONE |
| `AnchorDetector::with_patterns` with regex support | DONE |
| Configurable anchor detection | DONE |

### Phase 3: optimus-agent
| Item | Status |
|------|--------|
| `LlmProvider` trait with `complete()`, `model_name()`, `cost_config()` | DONE |
| `ChatProvider` (OpenAI-compatible) + `AnthropicProvider` | DONE |
| Auto-detect provider from `base_url` pattern | DONE |
| `discover_schema` via LLM + `discover_schema_offline` fallback | DONE |
| `infer_schema` heuristic (spatial right-neighbor + type inference) | DONE |
| `generate_guest_rust_code` via LLM + offline template | DONE |
| `optimus-guest` wired into codegen (`use optimus_guest::*`) | DONE |
| 4 prompt templates (schema, codegen, compile fix, extraction fix) | DONE |
| 2-loop compilation feedback (compile retries + extraction validation) | DONE |
| Artifact caching: `{cache_dir}/{layout_id}/` with WASM, source, manifest | DONE |
| `CostTracker` with per-layout + cumulative tracking | DONE |
| `TempArtifactGuard` (Drop-based cleanup on panic/error) | DONE |
| `compile_extraction_logic_sync` synchronous wrapper | DONE |

### Phase 4: optimus-runtime
| Item | Status |
|------|--------|
| `WasmHost` with `Engine` + `Arc<RwLock<HashMap>>` module cache | DONE |
| Shared `Arc<WasmHost>` across Rayon threads | DONE |
| `ExtractedRecord` dynamic via `HashMap<String, String>` | DONE |
| `build_dynamic_arrow_batch` with dynamic field names | DONE |
| `process_pdfs_summary` with error aggregation | DONE |
| `process_pdfs_streaming` with mpsc channels + chunked Arrow output | DONE |
| `process_pdfs_parallel` convenience wrapper | DONE |

### Phase 5: optimus-cli
| Item | Status |
|------|--------|
| 8 subcommands: extract, batch, cache, generate-grid, ingest, status, benchmark, watch | DONE |
| `--format json|arrow` with Arrow IPC output | DONE |
| `benchmark` with p50/p95/p99 latency + docs/sec throughput | DONE |
| `watch` with Ctrl+C graceful shutdown | DONE |
| `tracing-subscriber` initialization | DONE |
| `OptimusConfig::from_file()`, `from_env()`, `offline()` | DONE |

### Phase 6: Tauri + Frontend
| Item | Status |
|------|--------|
| 8 Tauri commands (7 async via `spawn_blocking`) | DONE |
| 12 event types including `compile-attempt` | DONE |
| Full pipeline integration (ingest → graph → hash → compile → extract → output) | DONE |
| `SpatialGraphView` receives spans from events | DONE |
| `SchemaEditor` with JSON validation + edit mode | DONE |
| `ArrowTableView` with Copy JSON/CSV export | DONE |
| `CacheBrowser` with detail panel, per-entry delete, version badge | DONE |
| `PdfDropZone` with two-step upload (select → Extract + Clear) | DONE |
| `cacheDir` uses `appDataDir()` | DONE |
| Async Tauri commands via `spawn_blocking` | DONE |
| Error banner + processing indicator | DONE |
| `tracing-subscriber` with env filter | DONE |
| Cache versioning (`CACHE_VERSION=2`) | DONE |
| `#[tracing::instrument]` on all public fns | DONE |
| 12 `.jsx`/`.js` duplicate files deleted | DONE |

---

## Phase 7: Multi-Step Extraction Wizard — PLANNED

### Problem
Current `extract_document_command` is monolithic — one Tauri invoke does everything. User has zero visibility into intermediate stages, cannot choose format, cannot use LLM for schema inference, cannot explicitly reuse cached WASM. The LLM integration (full pipeline with code generation, compile fix loop, extraction fix loop) is COMPLETELY implemented in `compiler.rs`, `llm.rs`, `codegen.rs`, `schema.rs` but NEVER REACHED from Tauri because `compile_extraction_logic_sync` hardcodes `provider: None`.

### Target Architecture

```
Upload → [INGEST] → choose/cache format → [INFER SCHEMA (LLM or heuristic)] → [COMPILE WASM (LLM loop)] → [EXTRACT] → data

Next same-format doc ─────────────────────────────────────────→ [EXTRACT (cached WASM)] → data (microseconds)
```

### Phase 7 Sub-Steps

#### 7.0 — LLM Provider Factory for Tauri
- **New file:** `src-tauri/src/llm_util.rs`
- `get_llm_provider()` reads `LlmConfig::from_env()` → `create_provider()` → returns `Option<Box<dyn LlmProvider>>`
- Provider is `Send + Sync`, can be shared via `Arc` in `spawn_blocking`
- If `OPTIMUS_LLM_API_KEY` not set → `None` → offline mode (current behavior)

#### 7.1 — Split Monolithic Pipeline into Stages (Backend)
- **`ingest_command(path)`** → `{ spans, grid, flat_graph, layout_id, bounding_box }`
  - Does NOT compile or extract — only span extraction + layout hashing
  - Returns ASCII grid for LLM preview, flat_graph for later use
- **`infer_schema_llm_command(grid)`** → `{ schema, provider_used, token_usage, fallback }`
  - Calls `schema::discover_schema(grid, provider)` if LLM available
  - Falls back to `infer_schema(spans)` if LLM unavailable or fails
  - `tokio::time::timeout(30s)` on LLM HTTP call
  - Returns token usage for cost display
- **`compile_module_command` (modified)** — accept `use_llm: bool`
  - If true: construct provider + cost tracker, call async `compile_extraction_logic` directly (no sync wrapper)
  - If false: use offline template fallback (current behavior)
- **`extract_cached_command(layout_id, cache_dir, flat_graph)`** → executes pre-compiled WASM
- **`extract_document_command` (backward compat)** — wraps above stages in sequence
- **`src-tauri/src/lib.rs`** — register 3 new commands

#### 7.2 — New Event Types
| Event | Payload | Purpose |
|-------|---------|---------|
| `pipeline:schema-inferred` | `{ schema, provider, token_usage, fallback }` | Schema from LLM/heuristic |
| `pipeline:code-generated` | `{ length, token_usage }` | Rust code generation complete |
| `pipeline:llm-fix` | `{ attempt, token_usage }` | LLM fix attempt applied |

#### 7.3 — Multi-Step UI Wizard (Frontend)
- **`UploadWizard.tsx` (new)** — container with step navigation
- **`Step0_Upload.tsx` (new)** — drop zone + spatial graph preview + format detection
- **`Step2_Schema.tsx` (new)** — schema source picker (heuristic / LLM / manual)
- **`Step3_Compile.tsx` (new)** — compile progress display with LLM retry visualization
- **`Step4_Extract.tsx` (new)** — extraction results + format caching confirmation
- **`SettingsPanel.tsx` (new)** — LLM API configuration (provider, model, cost limits)
- State machine in `App.tsx` signals: `PipelineStage = "idle" | "ingested" | "format-selected" | "schema-defined" | "compiled" | "extracted" | "error"`
- All pipeline data in-memory signals (no page routing — no state loss on tab switch)

#### 7.4 — Fast Path: Cached Format Bypass
- If `layout_id` is cached → `Step1_FormatPick` shows detected format card
- User clicks "Extract with cached format" → `extract_cached_command` → instant extraction (no schema, no compile)
- Next same-format documents: microsecond extraction via `WasmHost` module cache

#### 7.5 — Batch Extraction UI
- New "Batch" tab — drag multiple PDFs
- System groups by `layout_id`, extracts each group in parallel (Rayon-powered)
- Shows progress per group: cached = green (fast), new = amber (compiling)

#### 7.6 — Risk Mitigation
| Risk | Mitigation |
|------|-----------|
| LLM API key exposed in frontend | Key stays in Rust/Tauri, never serialized to JS |
| Orphan cargo processes on abort | `TempArtifactGuard` Drop cleanup + cancellation token |
| LLM returns invalid JSON | Parse error → fall back to heuristic schema |
| Rustc compilation hangs | `tokio::time::timeout(120s)` |
| Cost explosion from retries | `CostTracker::would_exceed` before every LLM call |
| Large spans/grid crash UI | Truncate display, paginate grid |
| Wizard state lost on tab switch | All state in App.tsx signals, persists across tabs |

---

## Dependency Status

| Crate | In | Purpose |
|-------|-----|---------|
| `tempfile` | `optimus-core` | Atomic temp file cleanup |
| `criterion` (dev) | `optimus-core` | Benchmarks |
| `sled` | `optimus-router` | Persistent layout DB |
| `regex` | `optimus-router` | Pattern-based anchor detection |
| `async-trait` | `optimus-agent` | LLM provider trait |
| `parking_lot` | `optimus-runtime` | Faster RwLock for module cache |
| `tracing` | all crates | Structured instrumentation |
| `wasmtime` | `optimus-agent`, `optimus-runtime` | WASM execution |
| `reqwest` | `optimus-agent` | HTTP client for LLM API |
| `tokio` | `optimus-agent`, `optimus-runtime`, `src-tauri` | Async runtime |
| `chrono` | `optimus-agent`, `optimus-runtime` | Timestamps |
| `toml` | `optimus-agent` | Config parsing |
| `futures` | `optimus-agent`, `optimus-runtime` | Async utilities |
| `clap` | `optimus-cli` | CLI argument parsing |
| `tracing-subscriber` | `optimus-cli`, `src-tauri` | Telemetry output |
| `notify` | `optimus-cli` | File watcher |
| `ctrlc` | `optimus-cli` | Graceful shutdown |
| `tauri` + plugins | `src-tauri` | Desktop framework |
| `solid-js` + vite | `src/` | Frontend framework |

---

## Risk Items

| Risk | Mitigation |
|------|-----------|
| `rustc` not installed with `wasm32-unknown-unknown` target | Pre-flight check in agent; auto-install via `rustup target add` |
| LLM generates uncompilable code >5 times | Fallback to template-based extraction |
| pdf_oxide fails on malformed PDFs | Graceful fallback to `lopdf` or `pdf-extract` crate |
| R-tree performance degrades on 100k+ spans | Page-bounded R-trees (one tree per page) |
| LLM API rate limits / cost spikes | Token budget per layout, exponential backoff, wait for retry-after |
| sled DB corruption under concurrent writes | `sled::Db::flush`, periodic compaction, `LayoutHealth` repair |
| Compilation stalls UI (no progress) | Event-based progress streaming (`pipeline:compile-attempt`) |

---

## Quick Reference: File Locations

| Component | Path | Key Files |
|-----------|------|-----------|
| Core ingestion | `optimus-core/src/lib.rs` | `TextSpan`, `SpatialGraph`, `GridConfig`, `ExtractionError` |
| Core tests | `optimus-core/tests/integration_test.rs` | 4 integration tests |
| Router | `optimus-router/src/lib.rs` | `LayoutDb`, `AnchorDetector`, `calculate_layout_id`, `LayoutHealth` |
| Agent schema | `optimus-agent/src/schema.rs` | `infer_schema`, `infer_field_type`, `discover_schema` (LLM) |
| Agent codegen | `optimus-agent/src/codegen.rs` | `build_code_template`, `generate_guest_rust_code` |
| Agent compiler | `optimus-agent/src/compiler.rs` | `compile_extraction_logic`, `run_wasm_module`, `TempArtifactGuard` |
| Agent LLM | `optimus-agent/src/llm.rs` | `LlmProvider`, `ChatProvider`, `AnthropicProvider` |
| Agent config | `optimus-agent/src/config.rs` | `LlmConfig`, `CompilationConfig`, `CostTracker` |
| Agent templates | `optimus-agent/src/templates.rs` | 4 prompt templates |
| Runtime | `optimus-runtime/src/lib.rs` | `WasmHost`, `ProcessSummary`, `process_pdfs_streaming` |
| Guest stdlib | `optimus-guest/src/lib.rs` | `parse_flat_graph`, `find_right_of`, `emit_json` |
| CLI | `optimus-cli/src/` | `main.rs` (8 subcommands), `commands.rs` |
| Tauri backend | `src-tauri/src/` | `lib.rs`, `commands.rs` (8 commands), `main.rs` |
| Frontend | `src/` | `App.tsx`, `components/*.tsx`, `lib/commands.ts`, `lib/events.ts` |
| Config | `optimus.toml` | Runtime + LLM configuration |
| Plan | `plan.md` | This file |
| Build | `Makefile` | 60+ targets |

---

## Execution Order

| # | Phase | Description | Status |
|---|-------|-------------|--------|
| 1 | Phase 1 | optimus-core | DONE |
| 2 | Phase 2 | optimus-router | DONE |
| 3 | Phase 3 | optimus-agent | DONE |
| 4 | Phase 4 | optimus-runtime + optimus-guest | DONE |
| 5 | Phase 5 | optimus-cli | DONE |
| 6 | Phase 6 | Tauri + Frontend | DONE |
| 7 | R1-R8 | Remaining work items | DONE |
| 8 | Polish | Schema inference, cache management, output export, tracing | DONE |
| 9 | Phase 7 | Multi-step extraction wizard (LLM integration, format library, parallel batch) | PLANNED |
