mod commands;
mod llm_util;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                if let Some(window) = app.get_webview_window("main") {
                    window.open_devtools();
                }
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
