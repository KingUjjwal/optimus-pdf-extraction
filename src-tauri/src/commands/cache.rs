use optimus_agent::{display_id, LayoutManifest};
use optimus_router::LayoutDb;
use tauri::AppHandle;

use super::emit_event;
use super::types::CacheEntry;
use super::validation::validate_cache_dir;

#[tauri::command]
#[tracing::instrument(level = "info", skip_all)]
pub fn get_cache_list_command(cache_dir: String) -> Result<Vec<CacheEntry>, String> {
    let cache_path = validate_cache_dir(&cache_dir)?;

    let layouts: Vec<CacheEntry> = if let Ok(db) = LayoutDb::open(&cache_path) {
        db.list_layouts()
            .into_iter()
            .map(|id| {
                let manifest_path = cache_path.join(&id).join("manifest.json");
                let (schema, model, compile_attempts, created_at, cache_version) =
                    if let Ok(json) = std::fs::read_to_string(&manifest_path) {
                        if let Ok(m) = serde_json::from_str::<LayoutManifest>(&json) {
                            (
                                m.schema,
                                m.model,
                                m.compile_attempts,
                                m.created_at,
                                m.cache_version,
                            )
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
pub fn get_cache_manifest_command(layout_id: String, cache_dir: String) -> Result<String, String> {
    if !super::validation::is_valid_layout_id(&layout_id) {
        return Err(format!("invalid layout_id: {}", layout_id));
    }
    let manifest_path = validate_cache_dir(&cache_dir)?
        .join(&layout_id)
        .join("manifest.json");

    std::fs::read_to_string(&manifest_path).map_err(|e| format!("read manifest: {}", e))
}

#[tauri::command]
#[tracing::instrument(level = "info", skip(app))]
pub fn clear_cache_command(cache_dir: String, app: AppHandle) -> Result<String, String> {
    let cache_path = validate_cache_dir(&cache_dir)?;

    if let Ok(db) = LayoutDb::open(&cache_path) {
        for id in db.list_layouts() {
            let _ = db.remove(&id);
            let artifact_dir = cache_path.join(&id);
            let _ = std::fs::remove_dir_all(&artifact_dir);
            let wasm_path = cache_path.join(format!("{}.wasm", id));
            let _ = std::fs::remove_file(&wasm_path);
        }
    }

    if let Ok(entries) = std::fs::read_dir(&cache_path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() && p.extension().is_some_and(|ext| ext == "wasm") {
                let _ = std::fs::remove_file(p);
            }
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
    if !super::validation::is_valid_layout_id(&layout_id) {
        return Err(format!("invalid layout_id: {}", layout_id));
    }
    let cache_path = validate_cache_dir(&cache_dir)?;

    if let Ok(db) = LayoutDb::open(&cache_path) {
        let _ = db.remove(&layout_id);
        let artifact_dir = cache_path.join(&layout_id);
        let _ = std::fs::remove_dir_all(&artifact_dir);
        let wasm_path = cache_path.join(format!("{}.wasm", layout_id));
        let _ = std::fs::remove_file(&wasm_path);
    }

    emit_event(
        &app,
        "cache:entry-deleted",
        serde_json::json!({"layout_id": layout_id}),
    );
    Ok(format!("Deleted cached layout {}", display_id(&layout_id)))
}
