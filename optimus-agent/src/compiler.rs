use crate::codegen;
use crate::config::{CompilationConfig, CostTracker, TokenUsage};
use crate::llm::LlmProvider;
use crate::observability::{record_llm_call, LlmCallHistory, LlmCallRecord, LlmCallType};
use crate::templates;
use anyhow::{anyhow, Result};
use optimus_core::{SpatialGraph, TextSpan};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::process::Command;

/// Per-layout async lock. The temp crate/target paths are namespaced by
/// `layout_id`, so two *different* layouts can compile concurrently; only
/// re-entrant compiles of the same layout need serializing. (The old global
/// lock serialized every compile behind every LLM fix call in a batch.)
fn layout_compile_lock(layout_id: &str) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
    let map = LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = map.lock().expect("layout lock map poisoned");
    guard
        .entry(layout_id.to_string())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

/// Builds a `cargo build --target wasm32-unknown-unknown --release` command for
/// the generated temp crate, sharing a workspace-wide wasm target dir so the
/// guest stdlib/std artifacts are reused across compiles instead of rebuilt
/// from scratch for every layout.
fn cargo_wasm_build(temp_crate_dir: &Path, wasm_target_dir: &Path) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.arg("build")
        .arg("--target")
        .arg("wasm32-unknown-unknown")
        .arg("--release")
        .current_dir(temp_crate_dir)
        .env("CARGO_TARGET_DIR", wasm_target_dir);
    cmd
}

/// Build a compact, deterministic "layout priors" hint for the LLM codegen
/// prompt: key-value pairs and transaction-table columns detected from the
/// document. The LLM trusts these instead of rediscovering geometry blind,
/// which cuts extraction-fix loops (and thus cost). Reuses precomputed
/// `DocumentFeatures` so the detectors are not re-run here.
fn build_layout_priors(features: &crate::features::DocumentFeatures) -> Option<String> {
    let mut sections: Vec<String> = Vec::new();

    if !features.key_value_pairs.is_empty() {
        let lines: Vec<String> = features
            .key_value_pairs
            .iter()
            .map(|p| format!("  {} -> {}", p.label, p.value))
            .collect();
        sections.push(format!("key_value_pairs:\n{}", lines.join("\n")));
    }

    if !features.transaction_columns.is_empty() {
        let lines: Vec<String> = features
            .transaction_columns
            .iter()
            .map(|(label, key)| format!("  {} (key={})", label, key))
            .collect();
        sections.push(format!("transaction_columns:\n{}", lines.join("\n")));
    }

    if let Some(cols) = &features.table_columns {
        if cols.len() >= 2 {
            let lines: Vec<String> = cols
                .iter()
                .map(|c| format!("  {} (x0={:.0}, x1={:.0})", c.header, c.x0, c.x1))
                .collect();
            sections.push(format!("table_columns:\n{}", lines.join("\n")));
        }
    }

    if sections.is_empty() {
        None
    } else {
        Some(sections.join("\n\n"))
    }
}

struct TempArtifactGuard {
    paths: Vec<PathBuf>,
    dirs: Vec<PathBuf>,
}

impl Drop for TempArtifactGuard {
    fn drop(&mut self) {
        for p in &self.paths {
            let _ = fs::remove_file(p);
        }
        for d in &self.dirs {
            let _ = fs::remove_dir_all(d);
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
/// Manifest describing a compiled WASM extraction module.
pub struct LayoutManifest {
    pub layout_id: String,
    pub schema: String,
    pub flat_graph: String,
    pub model: String,
    pub compile_attempts: u32,
    pub extraction_attempts: u32,
    pub llm_fix_count: u32,
    pub token_usage: TokenUsage,
    pub created_at: String,
    pub cache_version: u32,
}

pub const CACHE_VERSION: u32 = 3;

/// The `optimus-guest` stdlib source, embedded at agent-build time so the
/// generated temp crate is fully self-contained. A `path = "../optimus-guest"`
/// dependency resolves fine in the dev tree but does not exist inside a shipped
/// Tauri bundle, which would make every cache-miss compile fail on user machines.
const GUEST_SRC: &str = include_str!("../../optimus-guest/src/lib.rs");

/// Writes the temp crate's lib.rs as a fixed prelude plus the generated user
/// code, and writes the embedded guest stdlib as a sibling module. The `#[used]`
/// statics pin alloc/free_buf so rustc emits them as wasm exports even though the
/// generated `extract` never calls them directly.
fn write_guest_lib(src_dir: &std::path::Path, rust_code: &str) -> Result<()> {
    fs::write(src_dir.join("optimus_guest.rs"), GUEST_SRC)?;
    let prelude = r#"mod optimus_guest;
use optimus_guest::*;

#[used]
static __OPTIMUS_KEEP_ALLOC: extern "C" fn(usize) -> *mut u8 = optimus_guest::alloc;
#[used]
static __OPTIMUS_KEEP_FREE_BUF: unsafe extern "C" fn(*mut u8, usize) = optimus_guest::free_buf;
"#;
    fs::write(src_dir.join("lib.rs"), format!("{}{}", prelude, rust_code))?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
#[tracing::instrument(skip_all, fields(layout_id = %layout_id))]
pub async fn compile_extraction_logic(
    layout_id: &str,
    graph: &SpatialGraph,
    schema: &str,
    cache_dir: &Path,
    config: &CompilationConfig,
    provider: Option<&dyn LlmProvider>,
    cost_tracker: &mut CostTracker,
    history: Option<&LlmCallHistory>,
    event_tx: Option<&tokio::sync::mpsc::UnboundedSender<LlmCallRecord>>,
) -> Result<Vec<u8>> {
    compile_extraction_logic_with_flat_graph(
        layout_id,
        graph,
        None,
        schema,
        cache_dir,
        config,
        provider,
        cost_tracker,
        history,
        event_tx,
    )
    .await
}

/// Like `compile_extraction_logic`, but accepts an optional pre-serialized flat
/// graph (e.g. the one already produced during ingest) so it is not rebuilt.
#[allow(clippy::too_many_arguments)]
#[tracing::instrument(skip_all, fields(layout_id = %layout_id))]
pub async fn compile_extraction_logic_with_flat_graph(
    layout_id: &str,
    graph: &SpatialGraph,
    precomputed_flat_graph: Option<&str>,
    schema: &str,
    cache_dir: &Path,
    config: &CompilationConfig,
    provider: Option<&dyn LlmProvider>,
    cost_tracker: &mut CostTracker,
    history: Option<&LlmCallHistory>,
    event_tx: Option<&tokio::sync::mpsc::UnboundedSender<LlmCallRecord>>,
) -> Result<Vec<u8>> {
    // Serialize only re-entrant compiles of the *same* layout. Different layouts
    // use different temp dirs; cargo's own target-dir lock handles cross-layout
    // concurrency on the shared wasm target dir.
    let _compile_guard = layout_compile_lock(layout_id).lock_owned().await;

    fs::create_dir_all(cache_dir)?;
    let wasm_target_dir = cache_dir.join("wasm_target");
    let flat_graph = match precomputed_flat_graph {
        Some(fg) => fg.to_string(),
        None => crate::serialize_flat_graph(graph),
    };
    let graph_spans: Vec<TextSpan> = graph.nodes.iter().map(|n| n.span.clone()).collect();
    let features = crate::features::DocumentFeatures::compute(&graph_spans);
    let layout_priors = build_layout_priors(&features);

    let (mut rust_code, init_usage) = codegen::generate_guest_rust_code(
        schema,
        &flat_graph,
        layout_priors.as_deref(),
        provider,
        history,
        event_tx,
    )
    .await?;
    cost_tracker.record(layout_id, &init_usage);

    let model_name = provider
        .map(|p| p.model_name().to_string())
        .unwrap_or_else(|| "offline".into());
    let mut compile_attempts = 0u32;
    let mut extraction_attempts = 0u32;
    let mut llm_fix_count = 0u32;
    let mut total_usage = init_usage.clone();

    let rs_path = cache_dir.join(format!("temp_{}.rs", layout_id));
    let wasm_path = cache_dir.join(format!("temp_{}.wasm", layout_id));
    let temp_crate_dir = cache_dir.join(format!("temp_crate_{}", layout_id));

    let _guard = TempArtifactGuard {
        paths: vec![rs_path.clone(), wasm_path.clone()],
        dirs: vec![temp_crate_dir.clone()],
    };

    // === COMPILE LOOP ===
    tracing::info!(
        "compile pipeline start | layout={} | schema_keys={} | model={} | backoff={}ms",
        crate::display_id(layout_id),
        schema.len(),
        model_name,
        config.retry_backoff_ms,
    );
    let mut backoff_ms = config.retry_backoff_ms;
    loop {
        if cost_tracker.would_exceed(layout_id, &total_usage, config.max_cost_per_layout_cents) {
            tracing::error!("Cost limit exceeded for layout {}", layout_id);
            return Err(anyhow!("Cost limit exceeded for layout {}", layout_id));
        }

        let compile_start = Instant::now();
        tracing::info!(
            "cargo build attempt {}/{} for {}",
            compile_attempts + 1,
            config.max_compile_retries,
            crate::display_id(layout_id),
        );
        tracing::debug!("Rust source ({} bytes):\n{}", rust_code.len(), rust_code);

        fs::write(&rs_path, &rust_code)?;

        let temp_crate_dir = cache_dir.join(format!("temp_crate_{}", layout_id));
        let src_dir = temp_crate_dir.join("src");
        fs::create_dir_all(&src_dir)?;

        // Self-contained crate: the guest stdlib is embedded as a module, so no
        // path dependency (which would not exist in a bundled app) is needed.
        let cargo_toml = r#"
[package]
name = "temp_wasm_module"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[profile.release]
codegen-units = 1

[workspace]
"#;

        fs::write(temp_crate_dir.join("Cargo.toml"), cargo_toml)?;
        write_guest_lib(&src_dir, &rust_code)?;

        let output = cargo_wasm_build(&temp_crate_dir, &wasm_target_dir)
            .output()
            .await
            .map_err(|e| anyhow!("Failed to invoke cargo: {}", e))?;

        compile_attempts += 1;
        let elapsed = compile_start.elapsed();

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        if !stdout.is_empty() {
            tracing::debug!("cargo stdout ({} bytes):\n{}", stdout.len(), stdout);
        }
        if !stderr.is_empty() && output.status.success() {
            tracing::debug!(
                "cargo stderr/warnings ({} bytes):\n{}",
                stderr.len(),
                stderr
            );
        }

        if output.status.success() {
            let compiled_wasm =
                wasm_target_dir.join("wasm32-unknown-unknown/release/temp_wasm_module.wasm");
            if let Ok(bytes) = fs::read(&compiled_wasm) {
                match wasm_exports(&bytes) {
                    Ok(exports) if exports.iter().any(|e| e == "extract") => {
                        tracing::info!(
                            "cargo build OK → attempt {}/{}, {:.2}s, wasm={} bytes, exports={}",
                            compile_attempts,
                            config.max_compile_retries,
                            elapsed.as_secs_f64(),
                            bytes.len(),
                            exports.join(","),
                        );
                        fs::write(&wasm_path, bytes)?;
                        break;
                    }
                    Ok(exports) => {
                        tracing::warn!(
                            "cargo build OK but wasm lacks 'extract' export → attempt {}/{}, exports={:?}",
                            compile_attempts,
                            config.max_compile_retries,
                            exports,
                        );
                        stderr = format!(
                            "the wasm module exports [{}] but has no 'extract' export — the generated code produced an empty/incomplete module",
                            exports.join(", "),
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            "cargo build OK but could not inspect wasm exports → attempt {}/{}, error={}",
                            compile_attempts,
                            config.max_compile_retries,
                            e,
                        );
                        stderr = format!("could not inspect wasm exports: {}", e);
                    }
                }
            }
        }

        tracing::warn!(
            "cargo build FAIL → attempt {}/{}, {:.2}s\nstderr:\n{}",
            compile_attempts,
            config.max_compile_retries,
            elapsed.as_secs_f64(),
            stderr,
        );

        if compile_attempts >= config.max_compile_retries {
            let _ = fs::remove_file(&rs_path);
            let _ = fs::remove_file(&wasm_path);
            tracing::error!("Compilation failed after {} attempts", compile_attempts);
            return Err(anyhow!(
                "Compilation failed after {} attempts: {}",
                compile_attempts,
                stderr
            ));
        }

        if let Some(llm) = provider {
            llm_fix_count += 1;
            tracing::info!(
                "Invoking LLM fix for compilation errors (attempt {})",
                compile_attempts
            );
            let fix_prompt = templates::compilation_fix_user(&rust_code, &stderr);
            tracing::debug!("Compilation fix prompt ({} chars)", fix_prompt.len());
            let (fixed_code, usage) = record_llm_call(
                llm,
                LlmCallType::CompilationFix,
                templates::COMPILATION_FIX_SYSTEM,
                &fix_prompt,
                history,
                event_tx,
            )
            .await?;
            tracing::info!(
                "LLM fix returned: {} bytes | cost=${:.4}",
                fixed_code.len(),
                usage.estimated_cost_cents as f64 / 100.0,
            );
            rust_code = fixed_code;
            total_usage.add(&usage);
            cost_tracker.record(layout_id, &usage);
        } else {
            let _ = fs::remove_file(&rs_path);
            let _ = fs::remove_file(&wasm_path);
            tracing::error!("Compilation failed and no LLM provider available");
            return Err(anyhow!(
                "Compilation failed and no LLM provider for fix: {}",
                stderr
            ));
        }

        tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
        backoff_ms *= 2;
    }

    let mut wasm_bytes = fs::read(&wasm_path)?;

    // === EXTRACTION VALIDATION LOOP ===
    tracing::info!(
        "extraction validation start | layout={} | max_retries={}",
        crate::display_id(layout_id),
        config.max_extraction_retries,
    );
    loop {
        extraction_attempts += 1;

        let exec_error: Option<String>;
        let output_json = match run_wasm_module(&wasm_bytes, &flat_graph) {
            Ok(json) => {
                exec_error = None;
                json
            }
            Err(e) => {
                let msg = e.to_string();
                exec_error = Some(msg.clone());
                tracing::warn!(
                    "WASM execution failed for {} (attempt {}/{}): {}",
                    crate::display_id(layout_id),
                    extraction_attempts,
                    config.max_extraction_retries,
                    msg,
                );
                if extraction_attempts >= config.max_extraction_retries {
                    let _ = fs::remove_file(&rs_path);
                    let _ = fs::remove_file(&wasm_path);
                    return Err(anyhow!(
                        "WASM execution failed after {} extraction attempts: {}",
                        extraction_attempts,
                        msg,
                    ));
                }
                String::new()
            }
        };

        let schema_fields = codegen::parse_schema_fields(schema)?;
        if !output_json.is_empty() {
            let parsed: Result<serde_json::Value, _> = serde_json::from_str(&output_json);
            if let Ok(ref val) = parsed {
                if schema_matches(val, &schema_fields) && !extraction_is_empty(val, &schema_fields)
                {
                    tracing::info!(
                        "extraction OK → {} fields matched after {} validation attempts",
                        schema_fields.len(),
                        extraction_attempts,
                    );
                    break;
                }
                let missing: Vec<String> = schema_fields
                    .iter()
                    .filter(|f| !field_matches(val, f))
                    .map(|f| f.name.clone())
                    .collect();
                tracing::debug!(
                    "Extraction validation: missing fields: {:?}, empty={}, got: {}",
                    missing,
                    extraction_is_empty(val, &schema_fields),
                    &output_json[..output_json.len().min(200)],
                );
            }
        }

        // A fix is warranted when the module either crashed at runtime or
        // produced JSON that doesn't match the schema. Runtime crashes get the
        // actual wasm error so the LLM can repair the code instead of us
        // silently recompiling identical bytes.
        let fix_prompt = if let Some(err) = &exec_error {
            templates::extraction_runtime_fix_user(&rust_code, schema, err)
        } else if !output_json.is_empty() {
            templates::extraction_fix_user(&rust_code, schema, &output_json)
        } else {
            let _ = fs::remove_file(&rs_path);
            let _ = fs::remove_file(&wasm_path);
            return Err(anyhow!(
                "Extraction produced no output after {} attempts",
                extraction_attempts,
            ));
        };

        if extraction_attempts >= config.max_extraction_retries {
            let _ = fs::remove_file(&rs_path);
            let _ = fs::remove_file(&wasm_path);
            let got = if output_json.is_empty() {
                exec_error.as_deref().unwrap_or("no output").to_string()
            } else {
                output_json[..output_json.len().min(200)].to_string()
            };
            return Err(anyhow!(
                "Extraction validation failed after {} attempts: expected schema {} got {}",
                extraction_attempts,
                schema,
                got
            ));
        }

        if let Some(llm) = provider {
            llm_fix_count += 1;
            tracing::info!(
                "Invoking LLM fix for extraction (attempt {})",
                extraction_attempts,
            );
            tracing::debug!("Extraction fix prompt ({} chars)", fix_prompt.len());
            let (fixed_code, usage) = record_llm_call(
                llm,
                LlmCallType::ExtractionFix,
                templates::EXTRACTION_FIX_SYSTEM,
                &fix_prompt,
                history,
                event_tx,
            )
            .await?;
            tracing::info!(
                "LLM fix returned: {} bytes | cost=${:.4}",
                fixed_code.len(),
                usage.estimated_cost_cents as f64 / 100.0,
            );
            rust_code = fixed_code;
            total_usage.add(&usage);
            cost_tracker.record(layout_id, &usage);
        } else {
            let got = if output_json.is_empty() {
                exec_error.as_deref().unwrap_or("no output").to_string()
            } else {
                output_json[..output_json.len().min(200)].to_string()
            };
            tracing::error!("Extraction validation failed and no LLM provider available");
            return Err(anyhow!(
                "Extraction validation failed after {} attempts and no LLM provider for fix: expected schema {} got {}",
                extraction_attempts, schema, got,
            ));
        }

        // Recompile with fixed code
        tracing::info!(
            "recompiling after extraction fix (attempt {}/{})",
            extraction_attempts,
            config.max_extraction_retries,
        );
        let recompile_start = Instant::now();
        fs::write(&rs_path, &rust_code)?;
        let temp_crate_dir = cache_dir.join(format!("temp_crate_{}", layout_id));
        let src_dir = temp_crate_dir.join("src");
        fs::create_dir_all(&src_dir)?;
        write_guest_lib(&src_dir, &rust_code)?;

        let output = cargo_wasm_build(&temp_crate_dir, &wasm_target_dir)
            .output()
            .await
            .map_err(|e| anyhow!("Failed to invoke cargo for fix: {}", e))?;

        let rec_stderr = String::from_utf8_lossy(&output.stderr);
        let rec_elapsed = recompile_start.elapsed();

        if !output.status.success() {
            tracing::warn!(
                "Recompilation for fix FAILED ({:.2}s)\n{}",
                rec_elapsed.as_secs_f64(),
                rec_stderr,
            );
            // The loop head already counts this attempt; do not double-count.
            if extraction_attempts >= config.max_extraction_retries {
                let _ = fs::remove_file(&rs_path);
                let _ = fs::remove_file(&wasm_path);
                return Err(anyhow!("Recompilation for fix failed: {}", rec_stderr));
            }
            continue;
        }

        tracing::info!("recompile OK → {:.2}s", rec_elapsed.as_secs_f64());

        let compiled_wasm =
            wasm_target_dir.join("wasm32-unknown-unknown/release/temp_wasm_module.wasm");
        if let Ok(bytes) = fs::read(&compiled_wasm) {
            fs::write(&wasm_path, &bytes)?;
            wasm_bytes = bytes;
        } else {
            tracing::error!("Could not read recompiled WASM");
            return Err(anyhow!("Could not read recompiled WASM"));
        }
    }

    // Persist artifacts
    let final_wasm_path = cache_dir.join(format!("{}.wasm", layout_id));
    fs::write(&final_wasm_path, &wasm_bytes)?;

    let artifact_dir = cache_dir.join(layout_id);
    if let Err(e) = fs::create_dir_all(&artifact_dir) {
        tracing::warn!("Failed to create artifact dir {:?}: {}", artifact_dir, e);
    }
    if let Err(e) = fs::write(artifact_dir.join("source.rs"), &rust_code) {
        tracing::warn!("Failed to write source.rs to {:?}: {}", artifact_dir, e);
    }

    let manifest = LayoutManifest {
        layout_id: layout_id.to_string(),
        schema: schema.to_string(),
        flat_graph: flat_graph.clone(),
        model: model_name.clone(),
        compile_attempts,
        extraction_attempts,
        llm_fix_count,
        token_usage: total_usage.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
        cache_version: CACHE_VERSION,
    };
    if let Ok(json) = serde_json::to_string_pretty(&manifest) {
        if let Err(e) = fs::write(artifact_dir.join("manifest.json"), json) {
            tracing::warn!("Failed to write manifest.json to {:?}: {}", artifact_dir, e);
        }
    }

    tracing::info!(
        "compile pipeline DONE | layout={} | model={} | compile_attempts={} | extraction_attempts={} | wasm={} bytes | total_cost=${:.4}",
        crate::display_id(layout_id),
        model_name,
        compile_attempts,
        extraction_attempts,
        wasm_bytes.len(),
        total_usage.estimated_cost_cents as f64 / 100.0,
    );

    Ok(wasm_bytes)
}

/// Returns true when every scalar field is present and every array field is a
/// non-empty array whose rows contain all declared column keys.
fn schema_matches(val: &serde_json::Value, fields: &[codegen::SchemaField]) -> bool {
    fields.iter().all(|f| field_matches(val, f))
}

/// True when every scalar field came back as the guest's "Unknown"/empty
/// placeholder — the module compiled and ran but extracted nothing meaningful.
/// Without this, an all-"Unknown" result passed `schema_matches` (which only
/// checks presence) and got cached as a "successful" extractor.
///
/// Array fields are ignored: a statement may legitimately have zero rows. An
/// all-array or empty schema is likewise accepted (nothing to be empty about).
fn extraction_is_empty(val: &serde_json::Value, fields: &[codegen::SchemaField]) -> bool {
    let scalars: Vec<&codegen::SchemaField> =
        fields.iter().filter(|f| f.field_type != "array").collect();
    if scalars.is_empty() {
        return false;
    }
    scalars.iter().all(|f| match val.get(&f.name) {
        Some(serde_json::Value::String(s)) => {
            let t = s.trim();
            t.is_empty() || t.eq_ignore_ascii_case("unknown")
        }
        Some(serde_json::Value::Null) | None => true,
        // Any non-string scalar counts as an extracted value.
        Some(_) => false,
    })
}

fn field_matches(val: &serde_json::Value, f: &codegen::SchemaField) -> bool {
    let v = val.get(&f.name);
    if f.field_type == "array" {
        match v {
            // Empty arrays are valid (a statement may legitimately have zero rows).
            Some(serde_json::Value::Array(items)) => items.iter().all(|it| {
                it.as_object()
                    .map(|o| f.columns.iter().all(|(_, k)| o.contains_key(k)))
                    .unwrap_or(false)
            }),
            _ => false,
        }
    } else {
        v.is_some()
    }
}

/// Shared wasmtime engine used for compile-time validation. Engine creation
/// compiles the Cranelift ISA, so building one per validation attempt (and
/// another per export check) was significant overhead.
fn shared_engine() -> &'static wasmtime::Engine {
    static ENGINE: OnceLock<wasmtime::Engine> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let mut config = wasmtime::Config::new();
        config.cranelift_opt_level(wasmtime::OptLevel::Speed);
        config.consume_fuel(true);
        config.static_memory_maximum_size(100 * 1024 * 1024);
        wasmtime::Engine::new(&config).expect("failed to build wasmtime engine")
    })
}

/// Lists the exported symbol names of a compiled wasm module.
fn wasm_exports(wasm_bytes: &[u8]) -> Result<Vec<String>> {
    let module = wasmtime::Module::new(shared_engine(), wasm_bytes)?;
    Ok(module.exports().map(|e| e.name().to_string()).collect())
}

fn run_wasm_module(wasm_bytes: &[u8], flat_graph: &str) -> Result<String> {
    let engine = shared_engine();
    let module = wasmtime::Module::new(engine, wasm_bytes)?;
    let mut store = wasmtime::Store::new(engine, ());
    store.set_fuel(1_000_000_000u64)?;
    let linker = wasmtime::Linker::new(engine);
    let instance = linker.instantiate(&mut store, &module)?;

    let memory = instance
        .get_memory(&mut store, "memory")
        .ok_or_else(|| anyhow!("No memory export"))?;
    let alloc_fn = instance.get_typed_func::<u32, u32>(&mut store, "alloc")?;
    let extract_fn = instance.get_typed_func::<(u32, u32), u32>(&mut store, "extract")?;
    let free_fn = instance.get_typed_func::<(u32, u32), ()>(&mut store, "free_buf")?;

    let graph_bytes = flat_graph.as_bytes();
    let graph_len = graph_bytes.len() as u32;
    let guest_ptr = alloc_fn.call(&mut store, graph_len)?;
    memory.write(&mut store, guest_ptr as usize, graph_bytes)?;

    let result_ptr = extract_fn.call(&mut store, (guest_ptr, graph_len))?;
    if result_ptr == 0 {
        let _ = free_fn.call(&mut store, (guest_ptr, graph_len));
        return Err(anyhow!("extract returned null"));
    }

    let mut len_buf = [0u8; 4];
    memory.read(&store, result_ptr as usize, &mut len_buf)?;
    let result_len = u32::from_le_bytes(len_buf);

    if result_len > 10 * 1024 * 1024 {
        let _ = free_fn.call(&mut store, (guest_ptr, graph_len));
        return Err(anyhow!(
            "WASM result length {} exceeds 10MB limit",
            result_len
        ));
    }

    let mut result_bytes = vec![0u8; result_len as usize];
    memory.read(&store, (result_ptr + 4) as usize, &mut result_bytes)?;

    let _ = free_fn.call(&mut store, (result_ptr, result_len + 4));
    let _ = free_fn.call(&mut store, (guest_ptr, graph_len));

    Ok(String::from_utf8(result_bytes)?)
}

/// Synchronous wrapper for backward compatibility — spawns a tokio runtime.
/// Uses a shared OnceLock runtime to avoid creating a new runtime per call.
#[tracing::instrument(level = "info", skip(graph, cache_dir), fields(layout_id = %layout_id))]
pub fn compile_extraction_logic_sync(
    layout_id: &str,
    graph: &SpatialGraph,
    schema: &str,
    cache_dir: &Path,
) -> Result<Vec<u8>> {
    compile_extraction_logic_sync_with_flat_graph(layout_id, graph, None, schema, cache_dir)
}

/// Synchronous wrapper that also accepts a pre-serialized flat graph.
#[tracing::instrument(level = "info", skip(graph, cache_dir), fields(layout_id = %layout_id))]
pub fn compile_extraction_logic_sync_with_flat_graph(
    layout_id: &str,
    graph: &SpatialGraph,
    flat_graph: Option<&str>,
    schema: &str,
    cache_dir: &Path,
) -> Result<Vec<u8>> {
    use std::sync::OnceLock;
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    let rt =
        RT.get_or_init(|| tokio::runtime::Runtime::new().expect("failed to create tokio runtime"));
    let config = CompilationConfig::default();
    let mut tracker = CostTracker::default();
    rt.block_on(compile_extraction_logic_with_flat_graph(
        layout_id,
        graph,
        flat_graph,
        schema,
        cache_dir,
        &config,
        None,
        &mut tracker,
        None,
        None,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::SchemaField;

    fn scalar(name: &str) -> SchemaField {
        SchemaField {
            name: name.into(),
            field_type: "string".into(),
            columns: Vec::new(),
        }
    }

    fn array(name: &str) -> SchemaField {
        SchemaField {
            name: name.into(),
            field_type: "array".into(),
            columns: vec![("Date".into(), "date".into())],
        }
    }

    #[test]
    fn all_unknown_scalars_is_empty() {
        let fields = vec![scalar("invoice_number"), scalar("total")];
        let val = serde_json::json!({"invoice_number": "Unknown", "total": ""});
        assert!(extraction_is_empty(&val, &fields));
    }

    #[test]
    fn one_real_scalar_is_not_empty() {
        let fields = vec![scalar("invoice_number"), scalar("total")];
        let val = serde_json::json!({"invoice_number": "INV-1", "total": "Unknown"});
        assert!(!extraction_is_empty(&val, &fields));
    }

    #[test]
    fn array_only_schema_is_never_empty() {
        let fields = vec![array("transactions")];
        let val = serde_json::json!({"transactions": []});
        assert!(!extraction_is_empty(&val, &fields));
    }

    #[test]
    fn empty_schema_is_never_empty() {
        assert!(!extraction_is_empty(&serde_json::json!({}), &[]));
    }

    #[test]
    fn schema_matches_requires_all_fields() {
        let fields = vec![scalar("a"), scalar("b")];
        assert!(schema_matches(
            &serde_json::json!({"a": "1", "b": "2"}),
            &fields
        ));
        assert!(!schema_matches(&serde_json::json!({"a": "1"}), &fields));
    }

    #[test]
    fn array_field_requires_column_keys() {
        let field = array("transactions");
        let ok = serde_json::json!({"transactions": [{"date": "26-Jun-2025"}]});
        let bad = serde_json::json!({"transactions": [{"amount": "1"}]});
        assert!(field_matches(&ok, &field));
        assert!(!field_matches(&bad, &field));
    }
}
