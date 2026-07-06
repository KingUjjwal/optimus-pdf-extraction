# Optimus — Agent Guide

## Workspace
Multi-crate Rust workspace + Tauri v2 desktop + SolidJS frontend.

| Crate | Purpose |
|-------|---------|
| `optimus-core` | PDF ingestion, spatial graph (R-Tree), ASCII grid mock fallback under `#[cfg(test)]` |
| `optimus-router` | Layout fingerprinting (BLAKE3 anchor distance vectors), sled cache |
| `optimus-agent` | LLM + heuristic schema inference, Rust→WASM codegen, self-correcting compile loop |
| `optimus-guest` | `no_std` WASM guest stdlib: `parse_flat_graph`, `find_right_of`, `find_below`, `emit_json`, `alloc`/`free_buf` |
| `optimus-runtime` | Wasmtime host (Cranelift), Rayon parallel pipeline, Arrow RecordBatch output |
| `optimus-cli` | CLI (clap, 8 subcommands) |
| `src-tauri` | Tauri v2 backend — 14 registered commands, 15 pipeline event types |
| `src/` | SolidJS + Vite — 9 tabs, 10 components, `App.tsx` state machine |

## Key Commands
```sh
make dev-ui              # Tauri dev server (Rust + frontend HMR via bun run tauri dev)
make lint                # fmt-check + clippy + tsc
make test                # cargo test --workspace (needs PDF fixtures in optimus-core/tests/fixtures/)
make test-unit           # cargo test --workspace --lib
make test-integration    # cargo test --workspace --test '*'
make release             # lint → test → build --release
make bundle              # release → Tauri installer (.msi/.dmg/.deb)
make cli-extract PDF=a.pdf  # cargo run --bin optimus-cli -- extract ...
make cli-grid PDF=a.pdf     # ASCII grid dump for debugging
make bench               # criterion benchmarks (3 crates)
make clean-all           # nuclear: rm target/ cache/ dist/ node_modules/.vite graphify-out/
```
Phase 7 wizard commands in `commands.ts`: `ingestDocument`, `inferSchemaLLM`, `compileModuleLLM`, `extractCached`.

## Convention Gotchas

- **`#[tracing::instrument]`** on all public fns. `tracing-subscriber` with env-filter (`RUST_LOG=info`).
- **`extract_spans` mock fallback**: When `pdf_oxide` fails during tests, returns mock spans via `#[cfg(test)]` gate (`optimus-core/src/lib.rs:135`). Non-test builds return `Err`.
- **Multi-page PDF**: `extract_spans` loops all pages; y-coordinates offset by page height + 50px gap per page.
- **`infer_schema` is the LLM fallback** — when LLM fails/times out, `infer_schema(&spans)` does heuristic right-neighbor type detection.
- **`flat_graph` stored in `LayoutManifest`** — `extract_cached_command` reads it from `manifest.json`, not from schema.
- **Cache artifacts**: `{cache_dir}/{layout_id}/manifest.json` + `source.rs`; WASM at `{cache_dir}/{layout_id}.wasm`. `CACHE_VERSION=2` in `LayoutManifest`.
- **WASM compilation**: `cargo build --target wasm32-unknown-unknown --release` in a temp crate. Requires `rustup target add wasm32-unknown-unknown`.
- **LLM config precedence**: env vars `OPTIMUS_LLM_*` override `optimus.toml`; UI-saved config in `cache_dir/config.json` overrides both.
- **Frontend**: `bun install` (not npm). Events payloads are JSON-serialized strings — `JSON.parse(e.payload)`. `@tauri-apps/api` v2.
- **`.cargo/config.toml`**: Disables incremental + sets `codegen-units=1` for dev/test profiles.
- **`compile_extraction_logic` is async**; `compile_extraction_logic_sync` wraps it via `OnceLock<tokio::runtime::Runtime>`.
- **`WasmHost` shared via `Arc`** across Rayon threads. Module cache is `Arc<RwLock<HashMap<String, Module>>>`.
- **Tauri commands**: `async fn` → `tokio::task::spawn_blocking` for CPU work. Capabilities in `src-tauri/capabilities/default.json`.
- **`compileModuleLLM` arg order**: `(layoutId, spansJson, schema, cacheDir)` — swapping `layoutId`/`schema` is a known footgun.
- **Test fixtures**: Live PDFs in `optimus-core/tests/fixtures/` (`invoice.pdf`, `report.pdf`, `form.pdf`). Integration tests reference them at runtime.
