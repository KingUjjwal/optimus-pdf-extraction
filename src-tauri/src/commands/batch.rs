use optimus_runtime::ExtractedRecord;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::{AppHandle, State};

use super::emit_event;
use super::extract::extract_single;
use super::validation::validate_cache_dir;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchEntryResult {
    pub path: String,
    pub layout_id: String,
    pub was_cached: bool,
    pub success: bool,
    pub error: Option<String>,
    pub duration_ms: u64,
    pub fields: Option<ExtractedRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResultSummary {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub cached: usize,
    pub new_compilations: usize,
    pub entries: Vec<BatchEntryResult>,
}

#[tauri::command]
#[tracing::instrument(level = "info", skip(app, state))]
pub async fn batch_extract_command(
    paths: Vec<String>,
    cache_dir: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<BatchResultSummary, String> {
    let cache_path = validate_cache_dir(&cache_dir)?;
    if let Err(e) = std::fs::create_dir_all(&cache_path) {
        log::warn!("Failed to create directory {:?}: {}", cache_path, e);
    }
    let host = state.host.clone();
    let total = paths.len();

    emit_event(&app, "batch:start", serde_json::json!({"total": total}));

    // Extract in parallel across Rayon workers, sharing one WasmHost (Engine +
    // module cache). `par_iter().collect()` preserves input order.
    let emitter = app.clone();
    let entries: Vec<BatchEntryResult> = tokio::task::spawn_blocking(move || {
        paths
            .par_iter()
            .map(|path| {
                let t0 = std::time::Instant::now();
                let file_name = Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(path);

                emit_event(
                    &emitter,
                    "batch:file-start",
                    serde_json::json!({"path": path, "file": file_name}),
                );

                match extract_single(path, &cache_path, &host) {
                    Ok(result) => {
                        emit_event(
                            &emitter,
                            "batch:file-done",
                            serde_json::json!({
                                "path": path, "file": file_name,
                                "success": true, "cached": result.was_cached,
                            }),
                        );
                        BatchEntryResult {
                            path: path.clone(),
                            layout_id: result.layout_id,
                            was_cached: result.was_cached,
                            success: true,
                            error: None,
                            duration_ms: t0.elapsed().as_millis() as u64,
                            fields: Some(result.record),
                        }
                    }
                    Err(e) => {
                        emit_event(
                            &emitter,
                            "batch:file-done",
                            serde_json::json!({
                                "path": path, "file": file_name,
                                "success": false, "error": &e,
                            }),
                        );
                        BatchEntryResult {
                            path: path.clone(),
                            layout_id: String::new(),
                            was_cached: false,
                            success: false,
                            error: Some(e),
                            duration_ms: t0.elapsed().as_millis() as u64,
                            fields: None,
                        }
                    }
                }
            })
            .collect()
    })
    .await
    .map_err(|e| format!("task error: {}", e))?;

    let succeeded = entries.iter().filter(|e| e.success).count();
    let total_cached = entries.iter().filter(|e| e.was_cached).count();
    let failed = entries.len() - succeeded;
    let new_compilations = succeeded.saturating_sub(total_cached);

    emit_event(
        &app,
        "batch:done",
        serde_json::json!({
            "total": total, "succeeded": succeeded, "failed": failed,
        }),
    );

    Ok(BatchResultSummary {
        total,
        succeeded,
        failed,
        cached: total_cached,
        new_compilations,
        entries,
    })
}
