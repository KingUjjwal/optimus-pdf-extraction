use optimus_agent::{LlmCallHistory, LlmCallRecord};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfigPayload {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub max_tokens: u32,
}

#[tauri::command]
#[tracing::instrument(level = "info")]
pub fn save_llm_config_command(
    config: LlmConfigPayload,
    cache_dir: String,
) -> Result<String, String> {
    let cfg_path = PathBuf::from(&cache_dir).join("config.json");
    if let Err(e) = std::fs::create_dir_all(&cache_dir) {
        log::warn!("Failed to create directory {:?}: {}", cache_dir, e);
    }
    let json = serde_json::to_string_pretty(&config).map_err(|e| format!("serialize: {}", e))?;
    std::fs::write(&cfg_path, &json).map_err(|e| format!("write config: {}", e))?;
    tracing::info!("LLM config saved to {:?}", cfg_path);
    Ok("Config saved".into())
}

#[tauri::command]
#[tracing::instrument(level = "info")]
pub fn get_llm_history_command(cache_dir: String) -> Result<Vec<LlmCallRecord>, String> {
    let history_path = PathBuf::from(&cache_dir).join("llm_history.jsonl");
    let history = LlmCallHistory::with_persistence(1024, &history_path);
    Ok(history.recent(200))
}
