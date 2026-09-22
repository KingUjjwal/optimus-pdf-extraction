mod commands;
mod llm_util;
mod state;

use optimus_runtime::WasmHost;
use state::AppState;
use std::path::Path;
use tauri::Manager;

/// Compiles every cached `{layout_id}.wasm` into the shared host so the first
/// extraction for a known layout avoids module-compilation latency.
fn precompile_cached_modules(host: &WasmHost, cache_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(cache_dir) else {
        return;
    };
    let mut warmed = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("wasm") {
            continue;
        }
        let Some(layout_id) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if let Ok(bytes) = std::fs::read(&path) {
            if host.precompile(layout_id, &bytes).is_ok() {
                warmed += 1;
            }
        }
    }
    log::info!("precompiled {} cached WASM module(s)", warmed);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new())
        .setup(|app| {
            // Compile-time gate: `open_devtools` only exists under
            // `debug_assertions` (or the `devtools` feature), so a runtime
            // `cfg!` check would still fail to compile in release builds.
            #[cfg(debug_assertions)]
            if let Some(window) = app.get_webview_window("main") {
                window.open_devtools();
            }

            let cfg = optimus_agent::OptimusConfig::from_file(std::path::Path::new("optimus.toml"));

            // Honour [runtime].parallel_docs for the shared Rayon pool that
            // batch extraction runs on (0 = Rayon's CPU-count default).
            if cfg.runtime.parallel_docs > 0 {
                if let Err(e) = rayon::ThreadPoolBuilder::new()
                    .num_threads(cfg.runtime.parallel_docs)
                    .build_global()
                {
                    log::warn!("failed to set Rayon pool size: {}", e);
                }
            }

            // Optionally warm the shared module cache from already-compiled
            // modules in [cache].dir (should point at the UI cache directory).
            if cfg.runtime.precompile_on_startup {
                let host = app.state::<AppState>().host.clone();
                let cache_dir = std::path::PathBuf::from(&cfg.cache.dir);
                std::thread::spawn(move || precompile_cached_modules(&host, &cache_dir));
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::ingest_pdf_command,
            commands::extract_document_command,
            commands::discover_schema_command,
            commands::compile_module_command,
            commands::get_cache_list_command,
            commands::get_cache_manifest_command,
            commands::clear_cache_command,
            commands::delete_cache_entry_command,
            // Phase 7: multi-step wizard
            commands::ingest_command,
            commands::infer_schema_llm_command,
            commands::compile_module_llm_command,
            commands::extract_cached_command,
            commands::save_llm_config_command,
            commands::batch_extract_command,
            commands::get_llm_history_command,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Optimus");
}
