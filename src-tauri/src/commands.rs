use optimus_core::{extract_spans, build_spatial_graph, generate_ascii_grid, TextSpan};
use optimus_router::{calculate_layout_id, is_layout_cached, LayoutDb};
use optimus_agent::{
    serialize_flat_graph, discover_schema_llm, infer_schema,
    compile_extraction_logic_sync, LayoutManifest, CompilationConfig, CostTracker, TokenUsage,
    compiler::compile_extraction_logic,
};
use optimus_runtime::{WasmHost, ExtractedRecord};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

use crate::llm_util::get_llm_provider;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bounds {
    pub min_x: f32,
    pub max_x: f32,
    pub min_y: f32,
    pub max_y: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestResult {
    pub spans: Vec<TextSpan>,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestFullResult {
    pub spans: Vec<TextSpan>,
    pub count: usize,
    pub grid: String,
    pub flat_graph: String,
    pub layout_id: String,
    pub is_cached: bool,
    pub bounding_box: Bounds,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionResult {
    pub record: ExtractedRecord,
    pub spans: Vec<TextSpan>,
    pub layout_id: String,
    pub was_cached: bool,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaInferenceResult {
    pub schema: String,
    pub provider_used: String,
    pub token_usage: TokenUsage,
    pub fallback: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileResult {
    pub size_bytes: usize,
    pub compile_attempts: u32,
    pub extraction_attempts: u32,
    pub token_usage: TokenUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub layout_id: String,
    pub schema: String,
    pub model: String,
    pub compile_attempts: u32,
    pub created_at: String,
    pub cache_version: u32,
}

#[tracing::instrument(level = "debug", skip(app, payload))]
fn emit_event(app: &AppHandle, event: &str, payload: serde_json::Value) {
    let _ = app.emit(event, payload.to_string());
}

#[tauri::command]
#[tracing::instrument(level = "info", skip(app))]
pub async fn ingest_pdf_command(path: String, app: AppHandle) -> Result<IngestResult, String> {
    let app_clone = app.clone();
    tokio::task::spawn_blocking(move || {
        let t0 = std::time::Instant::now();
        emit_event(&app_clone, "pipeline:ingest-start", serde_json::json!({"path": path}));

        let spans = extract_spans(&path)
            .map_err(|e| format!("Failed to extract spans: {}", e))?;

        let count = spans.len();

        emit_event(&app_clone, "pipeline:ingest-done", serde_json::json!({
            "spans": count,
            "spans_data": spans,
            "duration_ms": t0.elapsed().as_millis()
        }));

        Ok(IngestResult { spans, count })
    }).await.map_err(|e| format!("task error: {}", e))?
}

#[tauri::command]
#[tracing::instrument(level = "info", skip(app))]
pub async fn extract_document_command(
    path: String,
    cache_dir: String,
    schema: Option<String>,
    app: AppHandle,
) -> Result<ExtractionResult, String> {
    let app_clone = app.clone();
    tokio::task::spawn_blocking(move || {
        let t0 = std::time::Instant::now();

        let cache_path = PathBuf::from(&cache_dir);
        let _ = std::fs::create_dir_all(&cache_path);

        emit_event(&app_clone, "pipeline:ingest-start", serde_json::json!({"path": path}));

        let spans = extract_spans(&path)
            .map_err(|e| format!("extract_spans: {}", e))?;

        emit_event(&app_clone, "pipeline:ingest-done", serde_json::json!({
            "spans": spans.len(),
            "spans_data": spans,
            "duration_ms": t0.elapsed().as_millis()
        }));

        let spans_clone = spans.clone();
        let t1 = std::time::Instant::now();
        let graph = build_spatial_graph(spans);
        let flat_graph = serialize_flat_graph(&graph);
        let core_spans: Vec<_> = graph.nodes.iter().map(|n| n.span.clone()).collect();

        emit_event(&app_clone, "pipeline:graph-built", serde_json::json!({
            "nodes": graph.nodes.len(),
            "duration_ms": t1.elapsed().as_millis()
        }));

        let t2 = std::time::Instant::now();
        let layout_id = calculate_layout_id(&core_spans);
        let was_cached = is_layout_cached(&layout_id, &cache_path);

        emit_event(&app_clone, "pipeline:layout-hash", serde_json::json!({
            "layout_id": layout_id,
            "is_cached": was_cached,
            "duration_ms": t2.elapsed().as_millis()
        }));

        let t3 = std::time::Instant::now();
        let wasm_bytes = if was_cached {
            std::fs::read(cache_path.join(format!("{}.wasm", layout_id)))
                .map_err(|e| format!("read cached wasm: {}", e))?
        } else {
            emit_event(&app_clone, "pipeline:compiling", serde_json::json!({
                "layout_id": layout_id,
                "attempt": 1,
                "stage": "compile"
            }));
            let final_schema = schema.unwrap_or_else(|| infer_schema(&core_spans));
            let result = compile_extraction_logic_sync(&layout_id, &graph, &final_schema, &cache_path);
            match result {
                Ok(bytes) => {
                    emit_event(&app_clone, "pipeline:compile-attempt", serde_json::json!({
                        "layout_id": layout_id,
                        "attempt": 1,
                        "status": "success"
                    }));
                    bytes
                }
                Err(e) => {
                    emit_event(&app_clone, "pipeline:compile-attempt", serde_json::json!({
                        "layout_id": layout_id,
                        "attempt": 1,
                        "status": "failed",
                        "errors": e.to_string()
                    }));
                    return Err(format!("compile: {}", e));
                }
            }
        };

        emit_event(&app_clone, "pipeline:compiled", serde_json::json!({
            "layout_id": layout_id,
            "size_bytes": wasm_bytes.len(),
            "duration_ms": t3.elapsed().as_millis()
        }));

        let t4 = std::time::Instant::now();
        let host = WasmHost::new();
        emit_event(&app_clone, "pipeline:extracting", serde_json::json!({"layout_id": layout_id}));

        let output_json = host
            .execute_extraction(&layout_id, &wasm_bytes, &flat_graph)
            .map_err(|e| format!("wasm extract: {}", e))?;

        let record: ExtractedRecord = serde_json::from_str(&output_json)
            .map_err(|e| format!("parse output: {}", e))?;

        let duration = t0.elapsed().as_millis() as u64;

        emit_event(&app_clone, "pipeline:extracted", serde_json::json!({
            "record": record,
            "duration_ms": t4.elapsed().as_millis()
        }));

        emit_event(&app_clone, "pipeline:done", serde_json::json!({
            "path": path,
            "total_duration_ms": duration
        }));

        Ok(ExtractionResult {
            record,
            spans: spans_clone,
            layout_id,
            was_cached,
            duration_ms: duration,
        })
    }).await.map_err(|e| format!("task error: {}", e))?
}

#[tauri::command]
#[tracing::instrument(level = "info", skip(app))]
pub async fn compile_module_command(
    layout_id: String,
    spans_json: String,
    schema: String,
    cache_dir: String,
    app: AppHandle,
) -> Result<String, String> {
    let app_clone = app.clone();
    tokio::task::spawn_blocking(move || {
        let spans: Vec<TextSpan> = serde_json::from_str(&spans_json)
            .map_err(|e| format!("parse spans: {}", e))?;

        let graph = build_spatial_graph(spans);
        let cache_path = PathBuf::from(&cache_dir);
        let _ = std::fs::create_dir_all(&cache_path);

        emit_event(&app_clone, "pipeline:compiling", serde_json::json!({
            "layout_id": layout_id,
            "attempt": 1,
            "stage": "compile"
        }));

        let wasm = compile_extraction_logic_sync(&layout_id, &graph, &schema, &cache_path)
            .map_err(|e| {
                emit_event(&app_clone, "pipeline:compile-attempt", serde_json::json!({
                    "layout_id": layout_id,
                    "attempt": 1,
                    "status": "failed",
                    "errors": e.to_string()
                }));
                format!("compile: {}", e)
            })?;

        emit_event(&app_clone, "pipeline:compile-attempt", serde_json::json!({
            "layout_id": layout_id,
            "attempt": 1,
            "status": "success"
        }));

        emit_event(&app_clone, "pipeline:compiled", serde_json::json!({
            "layout_id": layout_id,
            "size_bytes": wasm.len()
        }));

        Ok(format!("Module compiled: {} bytes", wasm.len()))
    }).await.map_err(|e| format!("task error: {}", e))?
}

#[tauri::command]
#[tracing::instrument(level = "info", skip_all)]
pub fn get_cache_list_command(cache_dir: String) -> Result<Vec<CacheEntry>, String> {
    let cache_path = PathBuf::from(&cache_dir);

    let layouts: Vec<CacheEntry> = if let Ok(db) = LayoutDb::open(&cache_path) {
        db.list_layouts()
            .into_iter()
            .map(|id| {
                let manifest_path = cache_path.join(&id).join("manifest.json");
                let (schema, model, compile_attempts, created_at, cache_version) =
                    if let Ok(json) = std::fs::read_to_string(&manifest_path) {
                        if let Ok(m) = serde_json::from_str::<LayoutManifest>(&json) {
                            (m.schema, m.model, m.compile_attempts, m.created_at, m.cache_version)
                        } else {
                            ("?".into(), "?".into(), 0, "?".into(), 0)
                        }
                    } else {
                        ("?".into(), "?".into(), 0, "?".into(), 0)
                    };
                CacheEntry {
                    layout_id: id,
                    schema,
                    model,
                    compile_attempts,
                    created_at,
                    cache_version,
                }
            })
            .collect()
    } else {
        Vec::new()
    };

    Ok(layouts)
}

#[tauri::command]
#[tracing::instrument(level = "info", skip_all)]
pub fn get_cache_manifest_command(
    layout_id: String,
    cache_dir: String,
) -> Result<String, String> {
    let manifest_path = PathBuf::from(&cache_dir)
        .join(&layout_id)
        .join("manifest.json");

    std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("read manifest: {}", e))
}

#[tauri::command]
#[tracing::instrument(level = "info", skip(app))]
pub fn clear_cache_command(cache_dir: String, app: AppHandle) -> Result<String, String> {
    let cache_path = PathBuf::from(&cache_dir);

    if let Ok(db) = LayoutDb::open(&cache_path) {
        for id in db.list_layouts() {
            let _ = db.remove(&id);
            let artifact_dir = cache_path.join(&id);
            let _ = std::fs::remove_dir_all(&artifact_dir);
        }
    }

    emit_event(&app, "cache:cleared", serde_json::json!({"dir": cache_dir}));
    Ok("Cache cleared".into())
}

#[tauri::command]
#[tracing::instrument(level = "info", skip(app))]
pub fn delete_cache_entry_command(
    layout_id: String,
    cache_dir: String,
    app: AppHandle,
) -> Result<String, String> {
    let cache_path = PathBuf::from(&cache_dir);

    if let Ok(db) = LayoutDb::open(&cache_path) {
        let _ = db.remove(&layout_id);
        let artifact_dir = cache_path.join(&layout_id);
        let _ = std::fs::remove_dir_all(&artifact_dir);
    }

    emit_event(&app, "cache:entry-deleted", serde_json::json!({"layout_id": layout_id}));
    Ok(format!("Deleted cached layout {}", optimus_agent::display_id(&layout_id)))
}

#[tauri::command]
#[tracing::instrument(level = "info", skip(app))]
pub async fn discover_schema_command(spans_json: String, custom_prompt: Option<String>, app: AppHandle) -> Result<String, String> {
    let app_clone = app.clone();
    let (spans, grid) = tokio::task::spawn_blocking(move || {
        let spans: Vec<TextSpan> = serde_json::from_str(&spans_json)
            .map_err(|e| format!("Failed to parse spans JSON: {}", e))?;
        let grid = generate_ascii_grid(&spans);
        Ok::<(Vec<TextSpan>, String), String>((spans, grid))
    }).await.map_err(|e| format!("Task error: {}", e))??;

    tracing::info!("Parsed {} spans, grid size: {} chars", spans.len(), grid.len());
    if custom_prompt.is_some() {
        tracing::info!("Using custom prompt for schema inference");
    }

    let provider = get_llm_provider();
    let schema = if let Some(provider) = provider {
        tracing::info!("Using LLM provider {} for schema inference", provider.model_name());

        match tokio::time::timeout(
            Duration::from_secs(30),
            discover_schema_llm(&grid, Some(provider.as_ref()), custom_prompt.as_deref())
        ).await {
            Ok(Ok((s, usage))) => {
                tracing::info!("LLM schema inference succeeded: {} chars, input={}, output={}", s.len(), usage.input_tokens, usage.output_tokens);
                emit_event(&app_clone, "schema-inferred", serde_json::json!({
                    "schema": &s,
                    "provider": provider.model_name(),
                    "fallback": false
                }));
                s
            }
            Ok(Err(e)) => {
                tracing::warn!("LLM schema inference failed: {}, falling back to heuristic", e);
                let fallback = infer_schema(&spans);
                tracing::info!("Heuristic schema: {} chars", fallback.len());
                emit_event(&app_clone, "schema-inferred", serde_json::json!({
                    "schema": &fallback,
                    "provider": "heuristic",
                    "fallback": true
                }));
                fallback
            }
            Err(_) => {
                tracing::warn!("LLM schema inference timed out, falling back to heuristic");
                let fallback = infer_schema(&spans);
                tracing::info!("Heuristic schema (timeout): {} chars", fallback.len());
                emit_event(&app_clone, "schema-inferred", serde_json::json!({
                    "schema": &fallback,
                    "provider": "heuristic",
                    "fallback": true
                }));
                fallback
            }
        }
    } else {
        tracing::info!("No LLM provider, using heuristic schema inference");
        let fallback = infer_schema(&spans);
        tracing::info!("Heuristic schema: {} chars", fallback.len());
        emit_event(&app_clone, "schema-inferred", serde_json::json!({
            "schema": &fallback,
            "provider": "heuristic",
            "fallback": false
        }));
        fallback
    };

    Ok(schema)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Phase 7: Multi-Step Extraction Wizard — new commands
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[tauri::command]
#[tracing::instrument(level = "info", skip(app))]
pub async fn ingest_command(
    path: String,
    cache_dir: String,
    app: AppHandle,
) -> Result<IngestFullResult, String> {
    let app_clone = app.clone();
    tokio::task::spawn_blocking(move || {
        let t0 = std::time::Instant::now();

        emit_event(&app_clone, "pipeline:ingest-start", serde_json::json!({"path": &path}));

        let spans = extract_spans(&path)
            .map_err(|e| format!("extract_spans: {}", e))?;

        emit_event(&app_clone, "pipeline:ingest-done", serde_json::json!({
            "spans": spans.len(),
            "duration_ms": t0.elapsed().as_millis()
        }));

        let graph = build_spatial_graph(spans.clone());
        let flat_graph = serialize_flat_graph(&graph);
        let core_spans: Vec<_> = graph.nodes.iter().map(|n| n.span.clone()).collect();

        emit_event(&app_clone, "pipeline:graph-built", serde_json::json!({
            "nodes": graph.nodes.len(),
            "duration_ms": t0.elapsed().as_millis()
        }));

        let layout_id = calculate_layout_id(&core_spans);
        let grid = generate_ascii_grid(&core_spans);

        let mut min_x = f32::MAX; let mut max_x = -f32::MAX;
        let mut min_y = f32::MAX; let mut max_y = -f32::MAX;
        for s in &core_spans {
            if s.x0 < min_x { min_x = s.x0; }
            if s.x1 > max_x { max_x = s.x1; }
            if s.y0 < min_y { min_y = s.y0; }
            if s.y1 > max_y { max_y = s.y1; }
        }

        let cache_path = PathBuf::from(&cache_dir);
        let _ = std::fs::create_dir_all(&cache_path);
        let is_cached = is_layout_cached(&layout_id, &cache_path);

        emit_event(&app_clone, "pipeline:layout-hash", serde_json::json!({
            "layout_id": &layout_id,
            "is_cached": is_cached,
            "duration_ms": t0.elapsed().as_millis()
        }));

        Ok(IngestFullResult {
            count: spans.len(),
            spans,
            grid,
            flat_graph,
            layout_id,
            is_cached,
            bounding_box: Bounds { min_x, max_x, min_y, max_y },
        })
    }).await.map_err(|e| format!("task error: {}", e))?
}

#[tauri::command]
#[tracing::instrument(level = "info", skip(app))]
pub async fn infer_schema_llm_command(
    grid: String,
    spans_json: String,
    app: AppHandle,
) -> Result<SchemaInferenceResult, String> {
    let provider = get_llm_provider();
    let provider_name = provider.as_ref().map(|p| p.model_name().to_string()).unwrap_or_else(|| "offline".into());

    if let Some(llm) = provider {
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            discover_schema_llm(&grid, Some(llm.as_ref()), None),
        ).await;

        match result {
            Ok(Ok((schema, usage))) => {
                emit_event(&app, "pipeline:schema-inferred", serde_json::json!({
                    "schema": &schema,
                    "provider": &provider_name,
                    "token_usage": &usage,
                    "fallback": false,
                }));
                return Ok(SchemaInferenceResult {
                    schema,
                    provider_used: provider_name,
                    token_usage: usage,
                    fallback: false,
                });
            }
            Ok(Err(e)) => {
                emit_event(&app, "pipeline:schema-inferred", serde_json::json!({
                    "error": e.to_string(),
                    "fallback": true,
                }));
                // LLM failed, fall through to heuristic
            }
            Err(_) => {
                emit_event(&app, "pipeline:schema-inferred", serde_json::json!({
                    "error": "LLM timeout after 30s",
                    "fallback": true,
                }));
                // Timeout, fall through to heuristic
            }
        }
    }

    // Fallback: heuristic schema from spans
    let spans: Vec<TextSpan> = serde_json::from_str(&spans_json).unwrap_or_default();
    let schema = if !spans.is_empty() {
        infer_schema(&spans)
    } else {
        serde_json::json!({"invoice_number":"string","date":"string","total":"string"}).to_string()
    };

    emit_event(&app, "pipeline:schema-inferred", serde_json::json!({
        "schema": &schema,
        "provider": "heuristic",
        "fallback": true,
    }));

    Ok(SchemaInferenceResult {
        schema,
        provider_used: "heuristic".into(),
        token_usage: TokenUsage::default(),
        fallback: true,
    })
}

#[tauri::command]
#[tracing::instrument(level = "info", skip(app))]
pub async fn compile_module_llm_command(
    layout_id: String,
    spans_json: String,
    schema: String,
    cache_dir: String,
    app: AppHandle,
) -> Result<CompileResult, String> {
    emit_event(&app, "pipeline:compiling", serde_json::json!({"layout_id": &layout_id, "attempt": 1, "stage": "compile"}));

    let cache_path = PathBuf::from(&cache_dir);
    let _ = std::fs::create_dir_all(&cache_path);

    let (graph, _flat_graph) = tokio::task::spawn_blocking(move || {
        let spans: Vec<TextSpan> = serde_json::from_str(&spans_json)
            .map_err(|e| format!("parse spans: {}", e))?;
        let graph = build_spatial_graph(spans);
        let flat_graph = serialize_flat_graph(&graph);
        Ok::<_, String>((graph, flat_graph))
    }).await.map_err(|e| format!("task error: {}", e))?.map_err(|e| e)?;

    let provider = get_llm_provider();
    let config = CompilationConfig::default();
    let mut cost_tracker = CostTracker::default();

    let wasm = match provider {
        Some(ref llm) => {
            compile_extraction_logic(
                &layout_id, &graph, &schema, &cache_path,
                &config, Some(llm.as_ref()), &mut cost_tracker,
            ).await.map_err(|e| {
                emit_event(&app, "pipeline:compile-attempt", serde_json::json!({
                    "layout_id": &layout_id,
                    "status": "failed",
                    "errors": e.to_string(),
                }));
                format!("LLM compile: {}", e)
            })?
        }
        None => {
            compile_extraction_logic_sync(&layout_id, &graph, &schema, &cache_path)
                .map_err(|e| format!("offline compile: {}", e))?
        }
    };

    emit_event(&app, "pipeline:compile-attempt", serde_json::json!({
        "layout_id": &layout_id,
        "attempt": 1,
        "status": "success"
    }));

    emit_event(&app, "pipeline:code-generated", serde_json::json!({
        "layout_id": &layout_id,
        "size_bytes": wasm.len(),
    }));

    let token_usage = cost_tracker.per_layout.get(&layout_id).cloned().unwrap_or_default();
    let compile_attempts = std::fs::read_to_string(cache_path.join(&layout_id).join("manifest.json"))
        .ok()
        .and_then(|j| serde_json::from_str::<LayoutManifest>(&j).ok())
        .map(|m| m.compile_attempts)
        .unwrap_or(1);

    Ok(CompileResult {
        size_bytes: wasm.len(),
        compile_attempts,
        extraction_attempts: 0u32,
        token_usage,
    })
}

#[tauri::command]
#[tracing::instrument(level = "info", skip(app))]
pub async fn extract_cached_command(
    layout_id: String,
    cache_dir: String,
    app: AppHandle,
) -> Result<ExtractionResult, String> {
    let t0 = std::time::Instant::now();

    let cache_path = PathBuf::from(&cache_dir);

    emit_event(&app, "pipeline:extracting", serde_json::json!({"layout_id": &layout_id}));

    let lid = layout_id.clone();
    let (wasm_bytes, flat_graph) = tokio::task::spawn_blocking(move || {
        let manifest_path = cache_path.join(&lid).join("manifest.json");
        let manifest_str = std::fs::read_to_string(&manifest_path)
            .map_err(|e| format!("read manifest: {}", e))?;

        let manifest: LayoutManifest = serde_json::from_str(&manifest_str)
            .map_err(|e| format!("parse manifest: {}", e))?;

        let wasm_bytes = std::fs::read(&cache_path.join(format!("{}.wasm", lid)))
            .map_err(|e| format!("read cached wasm: {}", e))?;

        Ok::<(Vec<u8>, String), String>((wasm_bytes, manifest.flat_graph))
    }).await.map_err(|e| format!("task error: {}", e))??;

    let host = WasmHost::new();
    let output_json = host.execute_extraction(&layout_id, &wasm_bytes, &flat_graph)
        .map_err(|e| format!("wasm extract: {}", e))?;

    let record: ExtractedRecord = serde_json::from_str(&output_json)
        .map_err(|e| format!("parse output: {}", e))?;

    let duration = t0.elapsed().as_millis() as u64;

    emit_event(&app, "pipeline:extracted", serde_json::json!({
        "record": &record,
        "duration_ms": duration
    }));

    emit_event(&app, "pipeline:done", serde_json::json!({
        "layout_id": &layout_id,
        "total_duration_ms": duration
    }));

    Ok(ExtractionResult {
        record,
        spans: vec![],
        layout_id: layout_id.clone(),
        was_cached: true,
        duration_ms: duration,
    })
}
