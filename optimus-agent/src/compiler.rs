use crate::codegen;
use crate::config::{CompilationConfig, CostTracker, TokenUsage};
use crate::llm::LlmProvider;
use crate::observability::{record_llm_call, LlmCallHistory, LlmCallRecord, LlmCallType};
use crate::templates;
use anyhow::{anyhow, Result};
use optimus_core::{SpatialGraph, TextSpan};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::process::Command;

/// Build a compact, deterministic "layout priors" hint for the LLM codegen
/// prompt: key-value pairs and transaction-table columns detected from the
/// graph's spans. The LLM trusts these instead of rediscovering geometry
/// blind, which cuts extraction-fix loops (and thus cost).
fn build_layout_priors(graph: &SpatialGraph) -> Option<String> {
    let spans: Vec<TextSpan> = graph.nodes.iter().map(|n| n.span.clone()).collect();
    if spans.is_empty() {
        return None;
    }

    let mut sections: Vec<String> = Vec::new();

    let kv = crate::table_kv::detect_key_value_pairs(&spans);
    if !kv.is_empty() {
        let lines: Vec<String> = kv
            .iter()
            .map(|p| format!("  {} -> {}", p.label, p.value))
            .collect();
        sections.push(format!("key_value_pairs:\n{}", lines.join("\n")));
    }

    let columns = crate::schema::detect_transactions_columns(&spans);
    if !columns.is_empty() {
        let lines: Vec<String> = columns
            .iter()
            .map(|(label, key)| format!("  {} (key={})", label, key))
            .collect();
        sections.push(format!("transaction_columns:\n{}", lines.join("\n")));
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

/// Writes the temp crate's lib.rs as a fixed prelude plus the generated user code.
/// The `#[used]` statics pin optimus_guest::alloc/free_buf so rustc emits them as
/// wasm exports even though the generated `extract` never calls them directly.
fn write_guest_lib(src_dir: &std::path::Path, rust_code: &str) -> Result<()> {
    let prelude = r#"use optimus_guest::*;

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
    use std::sync::OnceLock;
    static COMPILE_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    // Serialize compilation: the temp crate uses fixed paths per layout_id, and
    // cargo can't build the same target dir concurrently. A global lock keeps
    // parallel workers (Rayon batch) from clobbering each other's temp files.
    let _compile_guard = COMPILE_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;

    fs::create_dir_all(cache_dir)?;
    let flat_graph = crate::serialize_flat_graph(graph);
    let layout_priors = build_layout_priors(graph);

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

        let guest_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("optimus-guest")
            .display()
            .to_string()
            .replace("\\", "/");

        let cargo_toml = format!(
            r#"
[package]
name = "temp_wasm_module"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
optimus-guest = {{ path = "{}" }}

[profile.release]
codegen-units = 1

[workspace]
"#,
            guest_path
        );

        fs::write(temp_crate_dir.join("Cargo.toml"), cargo_toml)?;
        write_guest_lib(&src_dir, &rust_code)?;

        let output = Command::new("cargo")
            .arg("build")
            .arg("--target")
            .arg("wasm32-unknown-unknown")
            .arg("--release")
            .current_dir(&temp_crate_dir)
            .output()
            .await
            .map_err(|e| anyhow!("Failed to invoke cargo: {}", e))?;

        compile_attempts += 1;
        let elapsed = compile_start.elapsed();

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
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
                temp_crate_dir.join("target/wasm32-unknown-unknown/release/temp_wasm_module.wasm");
            if let Ok(bytes) = fs::read(&compiled_wasm) {
                tracing::info!(
                    "cargo build OK → attempt {}/{}, {:.2}s, wasm={} bytes",
                    compile_attempts,
                    config.max_compile_retries,
                    elapsed.as_secs_f64(),
                    bytes.len(),
                );
                fs::write(&wasm_path, bytes)?;
                break;
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

        let output_json = match run_wasm_module(&wasm_bytes, &flat_graph) {
            Ok(json) => json,
            Err(e) => {
                tracing::warn!(
                    "WASM execution failed for {} (attempt {}/{}): {}",
                    crate::display_id(layout_id),
                    extraction_attempts,
                    config.max_extraction_retries,
                    e,
                );
                if extraction_attempts >= config.max_extraction_retries {
                    let _ = fs::remove_file(&rs_path);
                    let _ = fs::remove_file(&wasm_path);
                    return Err(anyhow!(
                        "WASM execution failed after {} extraction attempts",
                        extraction_attempts
                    ));
                }
                String::new()
            }
        };

        if !output_json.is_empty() {
            let schema_fields = codegen::parse_schema_fields(schema)?;
            let parsed: Result<serde_json::Value, _> = serde_json::from_str(&output_json);
            if let Ok(ref val) = parsed {
                if schema_matches(val, &schema_fields) {
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
                    "Extraction validation: missing fields: {:?}, got: {}",
                    missing,
                    &output_json[..output_json.len().min(200)],
                );
            }

            if extraction_attempts >= config.max_extraction_retries {
                let _ = fs::remove_file(&rs_path);
                let _ = fs::remove_file(&wasm_path);
                return Err(anyhow!(
                    "Extraction validation failed after {} attempts: expected schema {} got {}",
                    extraction_attempts,
                    schema,
                    output_json
                ));
            }

            if let Some(llm) = provider {
                llm_fix_count += 1;
                tracing::info!(
                    "Invoking LLM fix for extraction (attempt {})",
                    extraction_attempts,
                );
                let fix_prompt = templates::extraction_fix_user(&rust_code, schema, &output_json);
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
                tracing::error!("Extraction validation failed and no LLM provider available");
                return Err(anyhow!(
                    "Extraction validation failed after {} attempts and no LLM provider for fix: expected schema {} got {}",
                    extraction_attempts, schema, &output_json[..output_json.len().min(200)],
                ));
            }
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

        let output = Command::new("cargo")
            .arg("build")
            .arg("--target")
            .arg("wasm32-unknown-unknown")
            .arg("--release")
            .current_dir(&temp_crate_dir)
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
            extraction_attempts += 1;
            if extraction_attempts >= config.max_extraction_retries {
                let _ = fs::remove_file(&rs_path);
                let _ = fs::remove_file(&wasm_path);
                return Err(anyhow!("Recompilation for fix failed: {}", rec_stderr));
            }
            continue;
        }

        tracing::info!("recompile OK → {:.2}s", rec_elapsed.as_secs_f64());

        let compiled_wasm =
            temp_crate_dir.join("target/wasm32-unknown-unknown/release/temp_wasm_module.wasm");
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

fn run_wasm_module(wasm_bytes: &[u8], flat_graph: &str) -> Result<String> {
    let mut config = wasmtime::Config::new();
    config.cranelift_opt_level(wasmtime::OptLevel::Speed);
    config.consume_fuel(true);
    config.static_memory_maximum_size(100 * 1024 * 1024);
    let engine = wasmtime::Engine::new(&config)?;
    let module = wasmtime::Module::new(&engine, wasm_bytes)?;
    let mut store = wasmtime::Store::new(&engine, ());
    store.set_fuel(1_000_000_000u64)?;
    let linker = wasmtime::Linker::new(&engine);
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
    use std::sync::OnceLock;
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    let rt =
        RT.get_or_init(|| tokio::runtime::Runtime::new().expect("failed to create tokio runtime"));
    let config = CompilationConfig::default();
    let mut tracker = CostTracker::default();
    rt.block_on(compile_extraction_logic(
        layout_id,
        graph,
        schema,
        cache_dir,
        &config,
        None,
        &mut tracker,
        None,
        None,
    ))
}
