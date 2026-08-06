use optimus_runtime::ExtractedRecord;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::AppHandle;

use super::emit_event;
use super::extract::extract_single;

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
#[tracing::instrument(level = "info", skip(app))]
pub async fn batch_extract_command(
    paths: Vec<String>,
    cache_dir: String,
    app: AppHandle,
) -> Result<BatchResultSummary, String> {
    let cache_path = PathBuf::from(&cache_dir);
    if let Err(e) = std::fs::create_dir_all(&cache_path) {
        log::warn!("Failed to create directory {:?}: {}", cache_path, e);
    }
    let mut entries = Vec::with_capacity(paths.len());
    let mut total_cached = 0usize;

    emit_event(
        &app,
        "batch:start",
        serde_json::json!({"total": paths.len()}),
    );

    for path in &paths {
        let t0 = std::time::Instant::now();
        let file_name = Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path);

        emit_event(
            &app,
            "batch:file-start",
            serde_json::json!({"path": path, "file": file_name}),
        );

        let t_file = std::time::Instant::now();
        match extract_single(path, &cache_path) {
            Ok(result) => {
                let was_cached = result.was_cached;
                if was_cached {
                    total_cached += 1;
                }
                entries.push(BatchEntryResult {
                    path: path.clone(),
                    layout_id: result.layout_id,
                    was_cached,
                    success: true,
                    error: None,
                    duration_ms: t_file.elapsed().as_millis() as u64,
                    fields: Some(result.record),
                });
                emit_event(
                    &app,
                    "batch:file-done",
                    serde_json::json!({
                        "path": path, "file": file_name, "success": true, "cached": was_cached,
                    }),
                );
            }
            Err(e) => {
                entries.push(BatchEntryResult {
                    path: path.clone(),
                    layout_id: String::new(),
                    was_cached: false,
                    success: false,
                    error: Some(e.clone()),
                    duration_ms: t0.elapsed().as_millis() as u64,
                    fields: None,
                });
                emit_event(
                    &app,
                    "batch:file-done",
                    serde_json::json!({
                        "path": path, "file": file_name, "success": false, "error": &e,
                    }),
                );
            }
        }
    }

    let succeeded = entries.iter().filter(|e| e.success).count();
    let failed = entries.len() - succeeded;
    let new_compilations = succeeded.saturating_sub(total_cached);

    emit_event(
        &app,
        "batch:done",
        serde_json::json!({
            "total": paths.len(), "succeeded": succeeded, "failed": failed,
        }),
    );

    Ok(BatchResultSummary {
        total: paths.len(),
        succeeded,
        failed,
        cached: total_cached,
        new_compilations,
        entries,
    })
}
