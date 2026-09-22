use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone)]
/// Top-level configuration for the Optimus pipeline.
pub struct OptimusConfig {
    pub core: CoreConfig,
    pub llm: LlmConfig,
    pub compilation: CompilationConfig,
    pub router: RouterConfig,
    pub runtime: RuntimeConfig,
    pub cache: CacheConfig,
}

#[derive(Debug, Clone)]
/// Core extraction/grid configuration.
pub struct CoreConfig {
    /// Maximum spans processed per document (extras are truncated).
    pub max_spans: usize,
    /// ASCII grid cell width in points.
    pub grid_x_bucket: u32,
    /// ASCII grid cell height in points.
    pub grid_y_bucket: u32,
    /// Append a `--font-stats--` footer to generated grids.
    pub include_font_size: bool,
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            max_spans: 100_000,
            grid_x_bucket: 8,
            grid_y_bucket: 15,
            // The wizard grid feeds the LLM schema prompt, where the font-stats
            // footer materially improves structure detection.
            include_font_size: true,
        }
    }
}

#[derive(Debug, Clone)]
/// Batch/runtime parallelism configuration.
pub struct RuntimeConfig {
    /// Parallel PDF workers (0 = auto / Rayon default).
    pub parallel_docs: usize,
    /// Records per Arrow batch.
    pub batch_size: usize,
    /// Precompile cached WASM modules into the shared host at startup.
    pub precompile_on_startup: bool,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            parallel_docs: 0,
            batch_size: 1024,
            precompile_on_startup: false,
        }
    }
}

#[derive(Debug, Clone)]
/// Cache location configuration.
pub struct CacheConfig {
    /// Directory for the layout DB and compiled WASM modules.
    pub dir: String,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            dir: "./optimus_cache".into(),
        }
    }
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
        let env_usize = |key: &str| {
            std::env::var(key)
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
        };
        let env_u32 = |key: &str| std::env::var(key).ok().and_then(|v| v.parse::<u32>().ok());
        let env_bool = |key: &str| {
            std::env::var(key)
                .ok()
                .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        };
        let core_defaults = CoreConfig::default();
        let runtime_defaults = RuntimeConfig::default();
        let cache_defaults = CacheConfig::default();
        Self {
            core: CoreConfig {
                max_spans: env_usize("OPTIMUS_MAX_SPANS").unwrap_or(core_defaults.max_spans),
                grid_x_bucket: env_u32("OPTIMUS_GRID_X_BUCKET")
                    .unwrap_or(core_defaults.grid_x_bucket),
                grid_y_bucket: env_u32("OPTIMUS_GRID_Y_BUCKET")
                    .unwrap_or(core_defaults.grid_y_bucket),
                include_font_size: env_bool("OPTIMUS_INCLUDE_FONT_SIZE")
                    .unwrap_or(core_defaults.include_font_size),
            },
            llm: LlmConfig {
                api_key,
                base_url: std::env::var("OPTIMUS_LLM_BASE_URL")
                    .unwrap_or_else(|_| "https://api.openai.com/v1".into()),
                model: std::env::var("OPTIMUS_LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into()),
                max_tokens_per_call: std::env::var("OPTIMUS_LLM_MAX_TOKENS")
                    .ok()
                    .and_then(|v| v.parse::<u32>().ok())
                    .unwrap_or(4096)
                    .clamp(1, 16384),
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
            runtime: RuntimeConfig {
                parallel_docs: env_usize("OPTIMUS_PARALLEL_DOCS")
                    .unwrap_or(runtime_defaults.parallel_docs),
                batch_size: env_usize("OPTIMUS_BATCH_SIZE").unwrap_or(runtime_defaults.batch_size),
                precompile_on_startup: env_bool("OPTIMUS_PRECOMPILE_ON_STARTUP")
                    .unwrap_or(runtime_defaults.precompile_on_startup),
            },
            cache: CacheConfig {
                dir: std::env::var("OPTIMUS_CACHE_DIR").unwrap_or(cache_defaults.dir),
            },
        }
    }

    #[tracing::instrument]
    pub fn offline() -> Self {
        Self {
            core: CoreConfig::default(),
            llm: LlmConfig::default(),
            compilation: CompilationConfig::default(),
            router: RouterConfig::default(),
            runtime: RuntimeConfig::default(),
            cache: CacheConfig::default(),
        }
    }

    /// Load configuration from an optimus.toml file, with env var overrides.
    #[tracing::instrument]
    pub fn from_file(path: &Path) -> Self {
        #[derive(serde::Deserialize)]
        struct FileConfig {
            core: Option<FileCoreConfig>,
            llm: Option<FileLlmConfig>,
            compilation: Option<FileCompilationConfig>,
            router: Option<FileRouterConfig>,
            runtime: Option<FileRuntimeConfig>,
            cache: Option<FileCacheConfig>,
        }
        #[derive(serde::Deserialize)]
        struct FileCoreConfig {
            max_spans: Option<usize>,
            grid_x_bucket: Option<u32>,
            grid_y_bucket: Option<u32>,
            include_font_size: Option<bool>,
        }
        #[derive(serde::Deserialize)]
        struct FileRuntimeConfig {
            parallel_docs: Option<usize>,
            batch_size: Option<usize>,
            precompile_on_startup: Option<bool>,
        }
        #[derive(serde::Deserialize)]
        struct FileCacheConfig {
            dir: Option<String>,
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
                            config.llm.max_tokens_per_call = v.clamp(1, 16384);
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
                if let Some(core) = file_cfg.core {
                    if std::env::var("OPTIMUS_MAX_SPANS").is_err() {
                        if let Some(v) = core.max_spans {
                            config.core.max_spans = v;
                        }
                    }
                    if std::env::var("OPTIMUS_GRID_X_BUCKET").is_err() {
                        if let Some(v) = core.grid_x_bucket {
                            config.core.grid_x_bucket = v;
                        }
                    }
                    if std::env::var("OPTIMUS_GRID_Y_BUCKET").is_err() {
                        if let Some(v) = core.grid_y_bucket {
                            config.core.grid_y_bucket = v;
                        }
                    }
                    if std::env::var("OPTIMUS_INCLUDE_FONT_SIZE").is_err() {
                        if let Some(v) = core.include_font_size {
                            config.core.include_font_size = v;
                        }
                    }
                }
                if let Some(rt) = file_cfg.runtime {
                    if std::env::var("OPTIMUS_PARALLEL_DOCS").is_err() {
                        if let Some(v) = rt.parallel_docs {
                            config.runtime.parallel_docs = v;
                        }
                    }
                    if std::env::var("OPTIMUS_BATCH_SIZE").is_err() {
                        if let Some(v) = rt.batch_size {
                            config.runtime.batch_size = v;
                        }
                    }
                    if std::env::var("OPTIMUS_PRECOMPILE_ON_STARTUP").is_err() {
                        if let Some(v) = rt.precompile_on_startup {
                            config.runtime.precompile_on_startup = v;
                        }
                    }
                }
                if let Some(cache) = file_cfg.cache {
                    if std::env::var("OPTIMUS_CACHE_DIR").is_err() {
                        if let Some(v) = cache.dir {
                            config.cache.dir = v;
                        }
                    }
                }
            }
        }

        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_file_parses_core_runtime_cache() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("optimus.toml");
        std::fs::write(
            &path,
            r#"
[core]
max_spans = 123
grid_x_bucket = 4
grid_y_bucket = 9
include_font_size = false

[runtime]
parallel_docs = 3
batch_size = 42
precompile_on_startup = true

[cache]
dir = "/tmp/optimus_test_cache"
"#,
        )
        .unwrap();

        let cfg = OptimusConfig::from_file(&path);
        assert_eq!(cfg.core.max_spans, 123);
        assert_eq!(cfg.core.grid_x_bucket, 4);
        assert_eq!(cfg.core.grid_y_bucket, 9);
        assert!(!cfg.core.include_font_size);
        assert_eq!(cfg.runtime.parallel_docs, 3);
        assert_eq!(cfg.runtime.batch_size, 42);
        assert!(cfg.runtime.precompile_on_startup);
        assert_eq!(cfg.cache.dir, "/tmp/optimus_test_cache");
    }

    #[test]
    fn missing_file_falls_back_to_defaults() {
        let cfg = OptimusConfig::from_file(Path::new("does_not_exist_xyz.toml"));
        assert_eq!(cfg.core.max_spans, CoreConfig::default().max_spans);
        assert_eq!(cfg.runtime.batch_size, RuntimeConfig::default().batch_size);
    }
}
