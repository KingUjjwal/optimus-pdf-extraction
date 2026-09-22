# Optimus Fix Plan — Complete

| # | Priority | Status | File | Line | Issue | Fix |
|---|----------|--------|------|------|-------|-----|
| 1 | P0 | [x] | Wizard.tsx | 96 | `compileModuleLLM(schema, lid, ...)` — arg order swapped. TS sig is `(layoutId, spansJson, schema, cacheDir)`. `lid` and `schema` flipped. | Swap to `compileModuleLLM(lid, JSON.stringify(spans), schema, cacheDir)` |
| 2 | P0 | [x] | Wizard.tsx | 97-98 | `result.cached`, `result.provider_used`, `result.fallback` — none exist on `CompileResult`. Runtime undefined. | Replace with `result.size_bytes`, `result.compile_attempts`. Remove dead fields. |
| 3 | P0 | [x] | commands.rs:661, compiler.rs | 661 | `extract_cached_command` passes `manifest.schema` (JSON `{"inv_num":"string"}`) as `flat_graph` to WASM. `parse_flat_graph` splits on `\|`, gets garbage → "Unknown". | Store `flat_graph` in `LayoutManifest`. Write to manifest.json during compile. Read back at line 661. |
| 4 | P0 | [x] | runtime/lib.rs:154, compiler.rs:398 | 154 | `alloc_fn` called for input buffer, `free_buf_fn` only called for output. Input memory never freed. | Free `guest_ptr` after extraction in both run_module and run_wasm_module. |
| 5 | P1 | [x] | core/lib.rs | 111 | `extract_spans(0)` — only page 0. Multi-page PDFs lose all pages after first. | Loop `0..doc.page_count()`, merge spans, offset y-coordinates by page height + gap. |
| 6 | P1 | [x] | commands.rs | 548 | LLM fallback returns `{"field":"string"}` hardcoded. `infer_schema` exists and works. | Use `infer_schema(&spans)` as fallback. Added `spans_json` param to infer_schema_llm_command. |
| 7 | P1 | [x] | schema.rs:39, lib.rs:40, cli, runtime, tauri | 39 | `discover_schema_offline(_ascii_grid)` ignores input, returns hardcoded invoice schema. | All callers now use `infer_schema(&spans)`. Added `discover_schema_from_spans` helper in lib.rs. Removed `discover_schema_offline` import from commands.rs. |
| 8 | P1 | [x] | compiler.rs | 411 | `Runtime::new()` per cache miss. Inside `spawn_blocking` now, but fragile. | Use `OnceLock<tokio::runtime::Runtime>` shared runtime. Single init. |
| 9 | P2 | [x] | config.rs | 152-204 | 50-line `if let` pyramid for TOML parsing. | Use `#[derive(Deserialize)] FileConfig` + `toml::from_str`. Cleaner matching. |
| 10 | P2 | [x] | codegen.rs | 119-122 | Schema + graph in system prompt, empty user prompt. | System = instructions only. User prompt = schema + spatial graph. |
| 11 | P2 | [x] | 10+ locations (compiler, cli, tauri) | — | `&layout_id[..16.min(layout_id.len())]` copy-pasted everywhere. | Added `pub fn display_id(id: &str) -> &str` in optimus-agent/lib.rs. Replaced all 7 call sites. |
| 12 | P2 | [x] | events.ts | 8-56 | Listeners registered in chain of `.then()` — fragile cleanup, drops events. | Rewrote with `Promise.all([...])`. All 15 listeners registered in parallel. |
| 13 | P2 | [x] | cli/commands.rs | 281 | `_ => ()` swallows all non-Create/Close watch events. | Log unknown events with `tracing::trace!`. |
| 14 | Roast | [x] | core/lib.rs | 179-303 | `build_spatial_graph` — 4 copy-pasted corridor queries (top/bottom/left/right). | Extracted `find_nearest_in_corridor` helper function. Called 4 times with different AABB corners. Dropped ~80 lines. |
| 15 | Roast | [x] | guest/lib.rs | 59-73 | `build_json` manually concatenates strings, no escaping. Quote/backslash → invalid JSON. | Added `escape_json` function handling `\"`, `\\`, `\n`, `\r`, `\t`, control chars. Applied to both keys and values. |
| 16 | Roast | [x] | llm.rs | 81-361 | `ChatProvider` + `AnthropicProvider` — identical struct fields + `estimate_tokens`/`compute_cost`. | Extracted `ProviderBase` with shared 7 fields + `estimate_tokens`/`compute_cost`/`fallback_usage` methods. Both providers wrap it. Dropped ~80 lines. |
| 17 | Roast | [x] | runtime/lib.rs | 76-101 | `execute_extraction` acquires read lock → drop → write lock → drop → read lock. 3 lock acquisitions. | Compile + execute locally, then insert into cache with single write lock. Removed re-read path. Dropped ~10 lines. |

**All 17/17 fixes complete — verified clean compilation with `cargo check`.**
