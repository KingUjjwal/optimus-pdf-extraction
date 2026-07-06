use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone)]
/// Top-level configuration for the Optimus pipeline.
pub struct OptimusConfig {
    pub llm: LlmConfig,
    pub compilation: CompilationConfig,
    pub router: RouterConfig,
}

#[derive(Debug, Clone)]
/// LLM provider configuration (API key, model, costs).
pub struct LlmConfig {
    pub api_key: Option<String>,
    pub base_url: String,
    pub model: String,
    pub max_tokens_per_call: u32,
    pub provider_enabled: bool,
    pub input_cost_per_1m: f64,
    pub output_cost_per_1m: f64,
}

#[derive(Debug, Clone)]
/// Configuration for the WASM compilation loop.
pub struct CompilationConfig {
    pub max_compile_retries: u32,
    pub max_extraction_retries: u32,
    pub max_cost_per_layout_cents: u32,
    pub retry_backoff_ms: u64,
}

#[derive(Debug, Clone, Default)]
/// Configuration for layout fingerprinting router.
pub struct RouterConfig {
    pub custom_patterns: Vec<String>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
/// Token usage and cost tracking for LLM calls.
pub struct TokenUsage {
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub estimated_cost_cents: u32,
}

impl TokenUsage {
    #[tracing::instrument(skip_all)]
    pub fn add(&mut self, other: &TokenUsage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.estimated_cost_cents += other.estimated_cost_cents;
    }
}

#[derive(Debug, Clone, Default)]
/// Per-layout cost tracking with budget enforcement.
pub struct CostTracker {
    pub per_layout: HashMap<String, TokenUsage>,
    pub cumulative: TokenUsage,
}

impl CostTracker {
    #[tracing::instrument(skip_all)]
    pub fn record(&mut self, layout_id: &str, usage: &TokenUsage) {
        self.cumulative.add(usage);
        self.per_layout
            .entry(layout_id.to_string())
            .or_default()
            .add(usage);
    }

    #[tracing::instrument(skip_all)]
    pub fn would_exceed(&self, layout_id: &str, additional: &TokenUsage, max_cents: u32) -> bool {
        let current = self
            .per_layout
            .get(layout_id)
            .map(|u| u.estimated_cost_cents)
            .unwrap_or(0);
        current + additional.estimated_cost_cents > max_cents
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: "https://api.openai.com/v1".into(),
            model: "gpt-4o-mini".into(),
            max_tokens_per_call: 4096,
            provider_enabled: false,
            input_cost_per_1m: 0.15,
            output_cost_per_1m: 0.60,
        }
    }
}

impl Default for CompilationConfig {
    fn default() -> Self {
        Self {
            max_compile_retries: 5,
            max_extraction_retries: 3,
            max_cost_per_layout_cents: 50,
            retry_backoff_ms: 1000,
        }
    }
}

impl OptimusConfig {
    #[tracing::instrument]
    pub fn from_env() -> Self {
        let api_key = std::env::var("OPTIMUS_LLM_API_KEY")
            .ok()
            .filter(|k| !k.is_empty());
        let provider_enabled = api_key.is_some();
        Self {
            llm: LlmConfig {
                api_key,
                base_url: std::env::var("OPTIMUS_LLM_BASE_URL")
                    .unwrap_or_else(|_| "https://api.openai.com/v1".into()),
                model: std::env::var("OPTIMUS_LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into()),
                max_tokens_per_call: std::env::var("OPTIMUS_LLM_MAX_TOKENS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(4096),
                provider_enabled,
                input_cost_per_1m: std::env::var("OPTIMUS_LLM_INPUT_COST")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.15),
                output_cost_per_1m: std::env::var("OPTIMUS_LLM_OUTPUT_COST")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.60),
            },
            compilation: CompilationConfig {
                max_compile_retries: std::env::var("OPTIMUS_MAX_COMPILE_RETRIES")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(5),
                max_extraction_retries: std::env::var("OPTIMUS_MAX_EXTRACTION_RETRIES")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(3),
                max_cost_per_layout_cents: std::env::var("OPTIMUS_MAX_COST_PER_LAYOUT")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(50),
                retry_backoff_ms: 1000,
            },
            router: RouterConfig::default(),
        }
    }

    #[tracing::instrument]
    pub fn offline() -> Self {
        Self {
            llm: LlmConfig::default(),
            compilation: CompilationConfig::default(),
            router: RouterConfig::default(),
        }
    }

    /// Load configuration from an optimus.toml file, with env var overrides.
    #[tracing::instrument]
    pub fn from_file(path: &Path) -> Self {
        #[derive(serde::Deserialize)]
        struct FileConfig {
            llm: Option<FileLlmConfig>,
            compilation: Option<FileCompilationConfig>,
            router: Option<FileRouterConfig>,
        }
        #[derive(serde::Deserialize)]
        struct FileLlmConfig {
            base_url: Option<String>,
            model: Option<String>,
            max_tokens_per_call: Option<u32>,
            input_cost_per_1m: Option<f64>,
            output_cost_per_1m: Option<f64>,
        }
        #[derive(serde::Deserialize)]
        struct FileCompilationConfig {
            max_compile_retries: Option<u32>,
            max_extraction_retries: Option<u32>,
            max_cost_per_layout_cents: Option<u32>,
        }
        #[derive(serde::Deserialize)]
        struct FileRouterConfig {
            custom_patterns: Option<Vec<String>>,
        }

        let mut config = Self::from_env();

        if let Ok(contents) = std::fs::read_to_string(path) {
            if let Ok(file_cfg) = toml::from_str::<FileConfig>(&contents) {
                if let Some(llm) = file_cfg.llm {
                    if std::env::var("OPTIMUS_LLM_BASE_URL").is_err() {
                        if let Some(v) = llm.base_url {
                            config.llm.base_url = v;
                        }
                    }
                    if std::env::var("OPTIMUS_LLM_MODEL").is_err() {
                        if let Some(v) = llm.model {
                            config.llm.model = v;
                        }
                    }
                    if std::env::var("OPTIMUS_LLM_MAX_TOKENS").is_err() {
                        if let Some(v) = llm.max_tokens_per_call {
                            config.llm.max_tokens_per_call = v;
                        }
                    }
                    if std::env::var("OPTIMUS_LLM_INPUT_COST").is_err() {
                        if let Some(v) = llm.input_cost_per_1m {
                            config.llm.input_cost_per_1m = v;
                        }
                    }
                    if std::env::var("OPTIMUS_LLM_OUTPUT_COST").is_err() {
                        if let Some(v) = llm.output_cost_per_1m {
                            config.llm.output_cost_per_1m = v;
                        }
                    }
                }
                if let Some(comp) = file_cfg.compilation {
                    if std::env::var("OPTIMUS_MAX_COMPILE_RETRIES").is_err() {
                        if let Some(v) = comp.max_compile_retries {
                            config.compilation.max_compile_retries = v;
                        }
                    }
                    if std::env::var("OPTIMUS_MAX_EXTRACTION_RETRIES").is_err() {
                        if let Some(v) = comp.max_extraction_retries {
                            config.compilation.max_extraction_retries = v;
                        }
                    }
                    if std::env::var("OPTIMUS_MAX_COST_PER_LAYOUT").is_err() {
                        if let Some(v) = comp.max_cost_per_layout_cents {
                            config.compilation.max_cost_per_layout_cents = v;
                        }
                    }
                }
                if let Some(router) = file_cfg.router {
                    if let Some(patterns) = router.custom_patterns {
                        config.router.custom_patterns = patterns;
                    }
                }
            }
        }

        config
    }
}
