use optimus_agent::LayoutManifest;
use optimus_core::extract_spans;
use optimus_runtime::{extract_from_spans, extract_from_spans_with_schema, WasmHost};
use std::path::Path;
use tauri::{AppHandle, State};

use super::emit_event;
use super::types::ExtractionResult;
use super::validation::{is_valid_layout_id, validate_cache_dir};
use crate::state::AppState;

#[tauri::command]
#[tracing::instrument(level = "info", skip(app, state))]
pub async fn extract_document_command(
    path: String,
    cache_dir: String,
    schema: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ExtractionResult, String> {
    let cache_path = validate_cache_dir(&cache_dir)?;
    let host = state.host.clone();
    let app_clone = app.clone();
    tokio::task::spawn_blocking(move || {
        let t0 = std::time::Instant::now();

        if let Err(e) = std::fs::create_dir_all(&cache_path) {
            log::warn!("Failed to create directory {:?}: {}", cache_path, e);
        }

        emit_event(
            &app_clone,
            "pipeline:ingest-start",
            serde_json::json!({"path": path}),
        );

        let spans = extract_spans(&path).map_err(|e| format!("extract_spans: {}", e))?;

        emit_event(
            &app_clone,
            "pipeline:ingest-done",
            serde_json::json!({
                "spans": spans.len(),
                "spans_data": spans,
                "duration_ms": t0.elapsed().as_millis()
            }),
        );

        let spans_clone = spans.clone();

        emit_event(
            &app_clone,
            "pipeline:extracting",
            serde_json::json!({"layout_id": ""}),
        );

        let (record, field_confidence, layout_id, was_cached) =
            extract_from_spans_with_schema(&host, &spans, &cache_path, schema.as_deref())
                .map_err(|e| format!("extraction: {}", e))?;

        let duration = t0.elapsed().as_millis() as u64;

        emit_event(
            &app_clone,
            "pipeline:extracted",
            serde_json::json!({
                "record": record,
                "duration_ms": duration
            }),
        );

        emit_event(
            &app_clone,
            "pipeline:done",
            serde_json::json!({
                "path": path,
                "total_duration_ms": duration
            }),
        );

        Ok(ExtractionResult {
            record,
            spans: spans_clone,
            layout_id,
            was_cached,
            duration_ms: duration,
            field_confidence,
        })
    })
    .await
    .map_err(|e| format!("task error: {}", e))?
}

#[tauri::command]
#[tracing::instrument(level = "info", skip(app, state))]
pub async fn extract_cached_command(
    layout_id: String,
    cache_dir: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ExtractionResult, String> {
    if !is_valid_layout_id(&layout_id) {
        return Err(format!("invalid layout_id: {}", layout_id));
    }
    let cache_path = validate_cache_dir(&cache_dir)?;
    let host = state.host.clone();
    let t0 = std::time::Instant::now();

    emit_event(
        &app,
        "pipeline:extracting",
        serde_json::json!({"layout_id": &layout_id}),
    );

    let lid = layout_id.clone();
    let (wasm_bytes, flat_graph) = tokio::task::spawn_blocking(move || {
        let manifest_path = cache_path.join(&lid).join("manifest.json");
        let manifest_str =
            std::fs::read_to_string(&manifest_path).map_err(|e| format!("read manifest: {}", e))?;

        let manifest: LayoutManifest =
            serde_json::from_str(&manifest_str).map_err(|e| format!("parse manifest: {}", e))?;

        if manifest.cache_version != optimus_agent::CACHE_VERSION {
            return Err(format!(
                "cached layout {} is stale (version {} != {}) — recompile required",
                optimus_agent::display_id(&lid),
                manifest.cache_version,
                optimus_agent::CACHE_VERSION
            ));
        }

        let wasm_bytes = std::fs::read(cache_path.join(format!("{}.wasm", lid)))
            .map_err(|e| format!("read cached wasm: {}", e))?;

        Ok::<(Vec<u8>, String), String>((wasm_bytes, manifest.flat_graph))
    })
    .await
    .map_err(|e| format!("task error: {}", e))??;

    let (record, field_confidence) = {
        let output_json = {
            let lid_exec = layout_id.clone();
            tokio::task::spawn_blocking(move || {
                host.execute_extraction(&lid_exec, &wasm_bytes, &flat_graph)
            })
            .await
            .map_err(|e| format!("task error: {}", e))?
            .map_err(|e| format!("wasm extract: {}", e))?
        };
        optimus_runtime::parse_extracted_with_confidence(&output_json)
            .map_err(|e| format!("parse output: {}", e))?
    };

    let duration = t0.elapsed().as_millis() as u64;

    emit_event(
        &app,
        "pipeline:extracted",
        serde_json::json!({
            "record": &record,
            "duration_ms": duration
        }),
    );

    emit_event(
        &app,
        "pipeline:done",
        serde_json::json!({
            "layout_id": &layout_id,
            "total_duration_ms": duration
        }),
    );

    Ok(ExtractionResult {
        record,
        spans: vec![],
        layout_id: layout_id.clone(),
        was_cached: true,
        duration_ms: duration,
        field_confidence,
    })
}

// Shared extraction logic used by both single and batch. The caller supplies a
// shared `WasmHost` so batch processing reuses one wasmtime Engine + module cache.
pub(super) fn extract_single(
    path: &str,
    cache_path: &Path,
    host: &WasmHost,
) -> Result<ExtractionResult, String> {
    let spans = extract_spans(path).map_err(|e| format!("extract_spans: {}", e))?;

    let (record, field_confidence, layout_id, was_cached) =
        extract_from_spans(host, &spans, cache_path).map_err(|e| format!("{}", e))?;

    Ok(ExtractionResult {
        record,
        spans,
        layout_id,
        was_cached,
        duration_ms: 0,
        field_confidence,
    })
}
