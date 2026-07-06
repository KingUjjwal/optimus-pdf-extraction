use optimus_agent::{create_provider, LlmProvider, OptimusConfig};
use std::path::Path;

fn load_llm_config(cache_dir: Option<&Path>) -> OptimusConfig {
    let mut config = OptimusConfig::from_env();

    // Override with saved config.json if it exists
    if let Some(dir) = cache_dir {
        let cfg_path = dir.join("config.json");
        if let Ok(json) = std::fs::read_to_string(&cfg_path) {
            if let Ok(saved) = serde_json::from_str::<serde_json::Value>(&json) {
                if let Some(api_key) = saved
                    .get("api_key")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                {
                    config.llm.api_key = Some(api_key.into());
                    config.llm.provider_enabled = true;
                }
                if let Some(base_url) = saved
                    .get("base_url")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                {
                    config.llm.base_url = base_url.into();
                }
                if let Some(model) = saved
                    .get("model")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                {
                    config.llm.model = model.into();
                }
                if let Some(v) = saved.get("max_tokens").and_then(|v| v.as_u64()) {
                    config.llm.max_tokens_per_call = v as u32;
                }
            }
        }
    }

    config
}

#[tracing::instrument]
pub fn get_llm_provider() -> Option<Box<dyn LlmProvider>> {
    let cache_dir = std::env::current_dir()
        .ok()
        .map(|d| d.join("optimus_cache"));
    let config = load_llm_config(cache_dir.as_deref()).llm;
    if !config.provider_enabled {
        return None;
    }
    create_provider(&config)
}

#[tracing::instrument]
pub fn get_llm_provider_with_cache_dir(cache_dir: &Path) -> Option<Box<dyn LlmProvider>> {
    let config = load_llm_config(Some(cache_dir)).llm;
    if !config.provider_enabled {
        return None;
    }
    create_provider(&config)
}
