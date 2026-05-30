# Optimus

**High-Throughput Spatial Layout Analyzer & Agentic Guest Compiler Loop**

Optimus combines **R-Tree spatial indexing**, **invariant layout fingerprinting (BLAKE3)**, **LLM + heuristic schema inference**, and **agentic JIT guest code compilation (Rust → WASM)** for fast, sandboxed, accurate document extraction.

Desktop app powered by **Tauri v2 + SolidJS**. CLI for batch/automation. ~43 tests, 6 Rust crates, 60+ Make targets.

---

## Architecture

```
┌───────────────────────────────────────────────────────────────────────┐
│                         OPTIMUS PIPELINE                               │
│                                                                       │
│  PDF ──► [Core] ──► [Router] ──► [Agent] ──► [Runtime] ──► Arrow     │
│            │            │            │            │                    │
│            ▼            ▼            ▼            ▼                    │
│         Spans        Layout ID    Schema +     Sandboxed               │
│         R-Tree       (BLAKE3)     WASM code    Extraction              │
│         Grid         Cache DB     LLM Loop      (wasmtime)             │
│                                                                       │
│  ┌─────────────┐                                               │
│  │ Frontend:   │  Tauri v2 + SolidJS + TypeScript               │
│  │  - Upload   │  8 commands, 12 event types, tab-based UI      │
│  │  - Schema   │  Spatial graph canvas, arrow table, logs       │
│  │  - Cache    │  Copy JSON/CSV export                          │
│  │  - Output   │                                               │
│  └─────────────┘                                               │
└───────────────────────────────────────────────────────────────────────┘
```

### Crate Breakdown

| Crate | Purpose | Key Capabilities |
|-------|---------|-----------------|
| `optimus-core` | Ingestion & Spatial Mapping | Memory-mapped PDF I/O, pdf_oxide span extraction, R-Tree (rstar) spatial graph, ASCII/Markdown grid downsampling |
| `optimus-router` | Layout Fingerprinting & Routing | Anchor detection (colon/heuristic/regex), BLAKE3 invariant hashing, sled layout DB, cache health validation/repair |
| `optimus-agent` | Agentic Compiler Loop | LLM + heuristic schema inference, dynamic guest Rust code generation, self-correcting compile LLM feedback loop, cost tracking, token budgets |
| `optimus-runtime` | High-Throughput Execution | Wasmtime sandbox (Cranelift), zero-copy FFI, pre-compilation module cache, Rayon parallel pipeline, Arrow RecordBatch output |
| `optimus-guest` | Guest Stdlib | `parse_flat_graph`, `find_right_of`, `find_below`, `emit_json`, `alloc`/`free_buf` — all `no_std` compatible |
| `optimus-cli` | CLI Interface | 8 subcommands (extract, batch, cache, grid, ingest, status, benchmark, watch), `--format json|arrow` |
| `src-tauri` | Tauri Desktop Backend | 8 commands (7 async), 12 event types, `tracing-subscriber`, progress streaming, cache management |
| `src/` | SolidJS Frontend | Tab-based UI, 7 components, spatial graph canvas, schema editor, cache browser, arrow table, log console |

---

## Features

### Spatial Ingestion
- Memory-mapped PDF loading (`memmap2`)
- `pdf_oxide` span extraction with `#[cfg(test)]` mock fallback
- O(N log N) R-Tree spatial indexing (`rstar`)
- Cardinal neighbor detection (top/bottom/left/right, 5px tolerance)
- ASCII + Markdown grid generation with configurable bucket sizes

### Layout Fingerprinting
- Anchor detection: colon-suffixed labels, all-uppercase, regex patterns, keyword matches
- Translation-invariant BLAKE3 hashing of top-5 anchor distance vectors
- Persistent cache via sled (B-tree key-value store)
- `LayoutHealth` validation: detects missing WASM/manifest/Sled entries
- `repair_layout()` auto-recovers from partial corruption

### Schema Inference
- **Heuristic**: `infer_schema(spans)` — finds `Label:` spans → nearest right-neighbor value → infers type (`date`/`number`/`string`)
- **LLM**: `discover_schema(grid, provider)` — sends ASCII grid to OpenAI/DeepSeek/Anthropic → JSON schema
- **Manual**: JSON editor with validation in Schema tab

### WASM Compilation
- LLM generates custom Rust guest code from schema + spatial graph
- `cargo build --target wasm32-unknown-unknown --release` compilation
- Self-correcting loop: rustc errors → LLM fix prompt → retry (exponential backoff, max 5 attempts)
- Extraction validation loop: runs WASM, checks all schema fields present, LLM fix if missing (max 3 attempts)
- Artifact caching: `{cache_dir}/{layout_id}/` with `{layout_id}.wasm`, `source.rs`, `manifest.json`
- `TempArtifactGuard`: auto-cleanup on panic/error
- Cache versioning (`CACHE_VERSION=2`): stale v1 entries flagged

### High-Throughput Extraction
- Wasmtime JIT execution (Cranelift `OptLevel::Speed`)
- Module pre-compilation cache (`Arc<RwLock<HashMap<String, Module>>`)
- Zero-copy FFI: alloc→write→extract→free_buf
- Rayon-parallel pipeline (`process_pdfs_parallel`, `process_pdfs_streaming`)
- Dynamic `ExtractedRecord` via `HashMap<String, String>`
- Apache Arrow RecordBatch output (dynamic schema)

### Desktop App (Tauri + SolidJS)
- Drag-and-drop PDF upload with spatial graph canvas
- Multi-tab layout: Ingest, Pipeline, Spatial Graph, Schema, Cache, Output
- Real-time pipeline event streaming (12 event types)
- Schema editor with JSON validation
- Cache browser with detail panel, per-entry delete, version badge (v1/v2)
- Arrow table with Copy JSON / Copy CSV export
- Error banner with click-to-dismiss
- Animated processing indicator
- Log console with auto-scroll + color-coded lines
- `cacheDir` auto-resolved to `appDataDir()`

---

## Quick Start

### Prerequisites
```bash
rustup target add wasm32-unknown-unknown
bun install             # or npm install
```

### Development
```bash
make dev                # Tauri desktop app (Rust + frontend HMR)
make dev-frontend       # Vite dev server only
make build              # Build entire workspace (debug)
make check              # Cargo check + tsc --noEmit
make test               # All tests (43 total)
make test-all           # Full CI gate: lint + tests
```

### CLI
```bash
make cli-extract PDF=path/to/file.pdf
make cli-batch INPUT=dir/ OUTPUT=out.arrow
make cli-ingest INPUT=dir/
make cli-status
make cli-benchmark COUNT=1000
make cli-grid PDF=path/to/file.pdf
make cli-watch DIR=dir/
make cli-cache-list
make cli-cache-clear
```

### Build & Release
```bash
make build-release      # Optimized build
make release            # Lint → test → build --release
make bundle             # Tauri installer (.msi/.dmg/.deb)
```

### Lint & Format
```bash
make lint               # clippy + fmt check + tsc
make fmt                # Auto-format Rust code
make clippy             # Clippy warnings-as-errors
```

### Clean
```bash
make clean              # Clean target/ + cache
make clean-all          # Nuclear: everything
make clean-cache        # Layout cache only
```

### Benchmark
```bash
make bench              # Criterion benchmarks
make bench-all          # Criterion + CLI benchmark (5000 iterations)
```

### Knowledge Graph (graphify)
```bash
make graphify           # Full extraction + clustering
make graphify-incremental  # Changed files only
make graphify-full      # Clear cache + full rebuild
```

### Stats
```bash
make stats              # LOC, crate versions, cache size
make size               # Binary sizes
make outdated           # Check for outdated deps
```

---

## Pipeline Flow

```
1. Upload PDF
   └─ extract_spans → TextSpan[] (x0,y0,x1,y1,text)

2. Build Spatial Graph
   └─ R-Tree corridor queries → SpatialGraph { nodes: SpatialNode[] }
   └─ Each node: nearest_top, nearest_bottom, nearest_left, nearest_right

3. Layout Fingerprint
   └─ Anchor detection (ends with :, uppercase, keywords)
   └─ Distance vectors (dx,dy) between top 5 anchors
   └─ BLAKE3 hash → layout_id

4. Cache Lookup
   ├─ Cached → read {layout_id}.wasm → jump to step 6
   └─ Miss   → continue to step 5

5. Compilation (cache miss only)
   ├─ Schema inference (heuristic or LLM)
   ├─ Generate guest Rust code (template or LLM)
   ├─ cargo build --target wasm32-unknown-unknown
   ├─ [if fail] LLM fix → retry (exponential backoff, max 5)
   ├─ WASM validation (wasmtime test run)
   ├─ [if fail] LLM fix → recompile (max 3)
   └─ Persist: {layout_id}.wasm + manifest.json

6. Extraction
   ├─ WasmHost::execute_extraction(layout_id, wasm_bytes, flat_graph)
   ├─ Guest: parse_flat_graph → find_right_of(labels) → emit_json
   └─ Host: read JSON → ExtractedRecord → Arrow RecordBatch

7. Output
   └─ Copy JSON / Copy CSV / Arrow IPC export
```

---

## Configuration

All configurable via `optimus.toml` or environment variables:

```toml
[core]
max_spans = 100000
grid_x_bucket = 8
grid_y_bucket = 15

[llm]
# Provider auto-detected from base_url
base_url = "https://api.openai.com/v1"     # OPTIMUS_LLM_BASE_URL
model = "gpt-4o-mini"                       # OPTIMUS_LLM_MODEL
# API key via env: OPTIMUS_LLM_API_KEY
max_tokens_per_call = 4096
input_cost_per_1m = 0.15
output_cost_per_1m = 0.60

[compilation]
max_compile_retries = 5                     # OPTIMUS_MAX_COMPILE_RETRIES
max_extraction_retries = 3                  # OPTIMUS_MAX_EXTRACTION_RETRIES
max_cost_per_layout_cents = 50              # OPTIMUS_MAX_COST_PER_LAYOUT

[runtime]
parallel_docs = 0                           # 0 = CPU count
batch_size = 1024
precompile_on_startup = true

[cache]
dir = "./optimus_cache"
```

---

## Test Suite

| Test Category | Count | Location |
|---------------|-------|----------|
| Unit (core) | 5 | `optimus-core/src/lib.rs` |
| Unit (router) | 6 | `optimus-router/src/lib.rs` |
| Unit (agent) | 6 | `optimus-agent/src/lib.rs` |
| Unit (runtime) | 3 | `optimus-runtime/src/lib.rs` |
| Unit (guest) | 5 | `optimus-guest/src/lib.rs` |
| Integration (core) | 4 | `optimus-core/tests/` |
| Integration (router) | 2 | `optimus-router/tests/` |
| Integration (runtime) | 9 | `optimus-runtime/tests/` |
| Integration (fixtures) | 3 | `optimus-runtime/tests/` |
| **Total** | **43** | |

---

## Stack

| Layer | Technology |
|-------|-----------|
| Desktop framework | Tauri v2 |
| Frontend | SolidJS + TypeScript + Vite |
| PDF ingestion | `memmap2` + `pdf_oxide` |
| Spatial indexing | `rstar` (R-Tree) |
| Hashing | BLAKE3 |
| Cache DB | sled |
| LLM | OpenAI-compatible / Anthropic (via reqwest) |
| Compilation | `rustc --target wasm32-unknown-unknown` |
| WASM runtime | `wasmtime` (Cranelift) |
| Parallelism | Rayon + tokio |
| Output | Apache Arrow |
| CLI | clap |
| Observability | tracing + tracing-subscriber |
| Config | TOML + env vars |
| Testing | Rust native + criterion (benchmarks) |

---

## File Map

```
Optimus/
├── plan.md                    # Full implementation plan + status
├── README.md                  # This file
├── Makefile                   # 60+ build/dev/test targets
├── optimus.toml               # Runtime configuration
├── Cargo.toml                 # Workspace root
├── package.json               # Frontend deps + scripts
├── vite.config.ts             # Vite config (SolidJS)
├── tsconfig.json              # TypeScript config
├── index.html                 # App entry point
│
├── optimus-core/              # Crate: Ingestion & Spatial Mapping
│   ├── src/lib.rs             # TextSpan, SpatialGraph, GridConfig, ExtractionError
│   └── tests/integration_test.rs
│
├── optimus-router/            # Crate: Layout Fingerprinting
│   ├── src/lib.rs             # LayoutDb, AnchorDetector, calculate_layout_id
│   └── tests/
│
├── optimus-agent/             # Crate: LLM & JIT Compiler
│   ├── src/lib.rs             # Re-exports
│   ├── src/schema.rs          # infer_schema, discover_schema (heuristic + LLM)
│   ├── src/codegen.rs         # build_code_template, generate_guest_rust_code
│   ├── src/compiler.rs        # compile_extraction_logic, TempArtifactGuard
│   ├── src/llm.rs             # LlmProvider, ChatProvider, AnthropicProvider
│   ├── src/config.rs          # LlmConfig, CompilationConfig, CostTracker
│   └── src/templates.rs       # 4 LLM prompt templates
│
├── optimus-runtime/           # Crate: Execution Engine
│   ├── src/lib.rs             # WasmHost, ProcessSummary, Arrow builders
│   └── tests/
│
├── optimus-guest/             # Crate: WASM Guest Stdlib
│   └── src/lib.rs             # parse_flat_graph, find_right_of, emit_json
│
├── optimus-cli/               # Crate: CLI
│   ├── src/main.rs            # 8 subcommands (clap)
│   └── src/commands.rs        # extract, batch, cache, grid, ingest, etc.
│
├── src-tauri/                 # Tauri Desktop App
│   ├── src/lib.rs             # App builder + command registration
│   ├── src/main.rs            # Entry point + tracing init
│   └── src/commands.rs        # 8 Tauri commands
│
├── src/                       # SolidJS Frontend
│   ├── App.tsx                # Root component, state machine, event wiring
│   ├── types.ts               # TypeScript interfaces
│   ├── styles.css             # Dark theme, BEM-like components
│   ├── main.tsx               # SolidJS entry
│   ├── lib/
│   │   ├── commands.ts        # 8 Tauri invoke wrappers
│   │   └── events.ts          # 12 pipeline event listeners
│   └── components/
│       ├── PdfDropZone.tsx     # Upload with Extract + Clear buttons
│       ├── PipelineInspector.tsx  # 8-step pipeline visualization
│       ├── SpatialGraphView.tsx   # Canvas-based span renderer
│       ├── SchemaEditor.tsx       # JSON schema edit/validate
│       ├── CacheBrowser.tsx       # Cache list + detail panel + delete
│       ├── ArrowTableView.tsx     # Results table + Copy JSON/CSV
│       └── LogConsole.tsx         # Color-coded event log
│
└── graphify-out/              # Knowledge graph outputs
    ├── graph.json             # Node + edge data
    ├── graph.html             # Interactive visualization
    └── GRAPH_REPORT.md        # Audit report
```

---

*Optimus — spatial document intelligence compiled on the fly at WebAssembly speeds.*
