use optimus_agent::{
    compile_extraction_logic_sync, compiler::compile_extraction_logic, serialize_flat_graph,
    CompilationConfig, CostTracker, LayoutManifest, LlmCallHistory, LlmCallRecord,
};
use optimus_core::{build_spatial_graph, TextSpan};
use tauri::{AppHandle, Emitter};

use super::emit_event;
use super::types::CompileResult;
use super::validation::{is_valid_layout_id, validate_cache_dir};
use crate::llm_util::get_llm_provider_with_cache_dir;

#[tauri::command]
#[tracing::instrument(level = "info", skip(app))]
pub async fn compile_module_command(
    layout_id: String,
    spans_json: String,
    schema: String,
    cache_dir: String,
    app: AppHandle,
) -> Result<String, String> {
    if !is_valid_layout_id(&layout_id) {
        return Err(format!("invalid layout_id: {}", layout_id));
    }
    let cache_path = validate_cache_dir(&cache_dir)?;
    let app_clone = app.clone();
    tokio::task::spawn_blocking(move || {
        let t0 = std::time::Instant::now();
        let spans: Vec<TextSpan> =
            serde_json::from_str(&spans_json).map_err(|e| format!("parse spans: {}", e))?;

        let graph = build_spatial_graph(spans);
        if let Err(e) = std::fs::create_dir_all(&cache_path) {
            log::warn!("Failed to create directory {:?}: {}", cache_path, e);
        }

        emit_event(
            &app_clone,
            "pipeline:compiling",
            serde_json::json!({
                "layout_id": layout_id,
                "attempt": 1,
                "stage": "compile"
            }),
        );

        let wasm = compile_extraction_logic_sync(&layout_id, &graph, &schema, &cache_path)
            .map_err(|e| {
                emit_event(
                    &app_clone,
                    "pipeline:compile-attempt",
                    serde_json::json!({
                        "layout_id": layout_id,
                        "attempt": 1,
                        "status": "failed",
                        "errors": e.to_string()
                    }),
                );
                format!("compile: {}", e)
            })?;

        emit_event(
            &app_clone,
            "pipeline:compile-attempt",
            serde_json::json!({
                "layout_id": layout_id,
                "attempt": 1,
                "status": "success"
            }),
        );

        emit_event(
            &app_clone,
            "pipeline:compiled",
            serde_json::json!({
                "layout_id": layout_id,
                "size_bytes": wasm.len(),
                "duration_ms": t0.elapsed().as_millis()
            }),
        );

        Ok(format!("Module compiled: {} bytes", wasm.len()))
    })
    .await
    .map_err(|e| format!("task error: {}", e))?
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
    if !is_valid_layout_id(&layout_id) {
        return Err(format!("invalid layout_id: {}", layout_id));
    }
    let cache_path = validate_cache_dir(&cache_dir)?;
    let t0 = std::time::Instant::now();
    emit_event(
        &app,
        "pipeline:compiling",
        serde_json::json!({"layout_id": &layout_id, "attempt": 1, "stage": "compile"}),
    );

    if let Err(e) = std::fs::create_dir_all(&cache_path) {
        log::warn!("Failed to create directory {:?}: {}", cache_path, e);
    }

    let (graph, _flat_graph) = tokio::task::spawn_blocking(move || {
        let spans: Vec<TextSpan> =
            serde_json::from_str(&spans_json).map_err(|e| format!("parse spans: {}", e))?;
        let graph = build_spatial_graph(spans);
        let flat_graph = serialize_flat_graph(&graph);
        Ok::<_, String>((graph, flat_graph))
    })
    .await
    .map_err(|e| format!("task error: {}", e))??;

    let history = LlmCallHistory::with_persistence(1024, &cache_path.join("llm_history.jsonl"));
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<LlmCallRecord>();
    let emitter_app = app.clone();
    tokio::spawn(async move {
        while let Some(record) = event_rx.recv().await {
            if let Ok(payload) = serde_json::to_string(&record) {
                let _ = emitter_app.emit("llm:call", payload);
            }
        }
    });

    let provider = get_llm_provider_with_cache_dir(&cache_path);
    let config = CompilationConfig::default();
    let mut cost_tracker = CostTracker::default();

    let wasm = match provider {
        Some(ref llm) => compile_extraction_logic(
            &layout_id,
            &graph,
            &schema,
            &cache_path,
            &config,
            Some(llm.as_ref()),
            &mut cost_tracker,
            Some(&history),
            Some(&event_tx),
        )
        .await
        .map_err(|e| {
            emit_event(
                &app,
                "pipeline:compile-attempt",
                serde_json::json!({
                    "layout_id": &layout_id,
                    "status": "failed",
                    "errors": e.to_string(),
                }),
            );
            format!("LLM compile: {}", e)
        })?,
        None => {
            let mut tracker = CostTracker::default();
            compile_extraction_logic(
                &layout_id,
                &graph,
                &schema,
                &cache_path,
                &CompilationConfig::default(),
                None,
                &mut tracker,
                None,
                None,
            )
            .await
            .map_err(|e| format!("offline compile: {}", e))?
        }
    };

    let token_usage = cost_tracker
        .per_layout
        .get(&layout_id)
        .cloned()
        .unwrap_or_default();
    let manifest = std::fs::read_to_string(cache_path.join(&layout_id).join("manifest.json"))
        .ok()
        .and_then(|j| serde_json::from_str::<LayoutManifest>(&j).ok());
    let compile_attempts = manifest.as_ref().map(|m| m.compile_attempts).unwrap_or(1);
    let extraction_attempts = manifest
        .as_ref()
        .map(|m| m.extraction_attempts)
        .unwrap_or(0);
    let llm_fix_attempts = manifest.as_ref().map(|m| m.llm_fix_count).unwrap_or(0);

    emit_event(
        &app,
        "pipeline:compile-attempt",
        serde_json::json!({
            "layout_id": &layout_id,
            "attempt": compile_attempts,
            "status": "success"
        }),
    );

    let duration_ms = t0.elapsed().as_millis() as u64;
    emit_event(
        &app,
        "pipeline:compiled",
        serde_json::json!({
            "layout_id": &layout_id,
            "size_bytes": wasm.len(),
            "duration_ms": duration_ms,
        }),
    );

    emit_event(
        &app,
        "pipeline:code-generated",
        serde_json::json!({
            "layout_id": &layout_id,
            "size_bytes": wasm.len(),
        }),
    );

    if llm_fix_attempts > 0 {
        emit_event(
            &app,
            "pipeline:llm-fix",
            serde_json::json!({
                "layout_id": &layout_id,
                "llm_fix_attempts": llm_fix_attempts,
                "token_usage": &token_usage,
            }),
        );
    }

    emit_event(
        &app,
        "pipeline:done",
        serde_json::json!({
            "layout_id": &layout_id,
            "total_duration_ms": duration_ms,
        }),
    );

    Ok(CompileResult {
        size_bytes: wasm.len(),
        compile_attempts,
        extraction_attempts,
        llm_fix_attempts,
        token_usage,
    })
}
