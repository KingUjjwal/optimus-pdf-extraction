use optimus_agent::{OptimusConfig, create_provider, LlmProvider};

pub fn get_llm_provider() -> Option<Box<dyn LlmProvider>> {
    let config = OptimusConfig::from_env().llm;
    if !config.provider_enabled {
        return None;
    }
    create_provider(&config)
}
