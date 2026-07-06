use crate::config::TokenUsage;
use crate::llm::LlmProvider;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LlmCallType {
    SchemaDiscovery,
    CodeGeneration,
    CompilationFix,
    ExtractionFix,
}

impl std::fmt::Display for LlmCallType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SchemaDiscovery => write!(f, "schema_discovery"),
            Self::CodeGeneration => write!(f, "code_generation"),
            Self::CompilationFix => write!(f, "compilation_fix"),
            Self::ExtractionFix => write!(f, "extraction_fix"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LlmErrorKind {
    Timeout,
    Auth,
    RateLimit,
    ParseError,
    Other(String),
}

impl std::fmt::Display for LlmErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout => write!(f, "timeout"),
            Self::Auth => write!(f, "auth"),
            Self::RateLimit => write!(f, "rate_limit"),
            Self::ParseError => write!(f, "parse_error"),
            Self::Other(s) => write!(f, "other:{}", s),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A recorded LLM API call with token usage and error info.
pub struct LlmCallRecord {
    pub call_type: LlmCallType,
    pub model: String,
    pub system_chars: usize,
    pub user_chars: usize,
    pub response_chars: usize,
    pub latency_ms: u64,
    pub token_usage: TokenUsage,
    pub success: bool,
    pub error_kind: Option<LlmErrorKind>,
    pub error_message: Option<String>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Aggregated statistics across LLM calls.
pub struct LlmAggregateStats {
    pub total_calls: u64,
    pub successful_calls: u64,
    pub failed_calls: u64,
    pub total_input_tokens: usize,
    pub total_output_tokens: usize,
    pub total_cost_cents: u32,
    pub avg_latency_ms: f64,
}

/// In-memory and persistent LLM call history.
pub struct LlmCallHistory {
    inner: Arc<Mutex<VecDeque<LlmCallRecord>>>,
    max_entries: usize,
    persist_path: Option<std::path::PathBuf>,
}

impl LlmCallHistory {
    #[tracing::instrument(skip_all)]
    pub fn new(max_entries: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::new())),
            max_entries,
            persist_path: None,
        }
    }

    #[tracing::instrument(skip_all)]
    pub fn with_persistence(max_entries: usize, path: &Path) -> Self {
        let mut history = Self::new(max_entries);
        history.load_from_disk(path);
        history.persist_path = Some(path.to_path_buf());
        history
    }

    fn load_from_disk(&self, path: &Path) {
        if !path.exists() {
            return;
        }
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return,
        };
        let mut inner = self.inner.lock().unwrap();
        for line in content.lines() {
            if let Ok(record) = serde_json::from_str::<LlmCallRecord>(line) {
                inner.push_back(record);
                if inner.len() > self.max_entries {
                    inner.pop_front();
                }
            }
        }
    }

    #[tracing::instrument(skip_all)]
    pub fn push(&self, record: LlmCallRecord) {
        {
            let mut inner = self.inner.lock().unwrap();
            inner.push_back(record.clone());
            while inner.len() > self.max_entries {
                inner.pop_front();
            }
        }
        if let Some(ref path) = self.persist_path {
            if let Ok(json) = serde_json::to_string(&record) {
                if let Err(e) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .and_then(|mut f| writeln!(f, "{}", json))
                {
                    tracing::warn!("Failed to persist LLM call record: {}", e);
                }
            }
        }
    }

    #[tracing::instrument(skip_all)]
    pub fn recent(&self, n: usize) -> Vec<LlmCallRecord> {
        let inner = self.inner.lock().unwrap();
        inner.iter().rev().take(n).cloned().collect()
    }

    #[tracing::instrument(skip_all)]
    pub fn aggregate(&self) -> LlmAggregateStats {
        let inner = self.inner.lock().unwrap();
        let total_calls = inner.len() as u64;
        let successful_calls = inner.iter().filter(|r| r.success).count() as u64;
        let failed_calls = total_calls - successful_calls;
        let total_input_tokens = inner.iter().map(|r| r.token_usage.input_tokens).sum();
        let total_output_tokens = inner.iter().map(|r| r.token_usage.output_tokens).sum();
        let total_cost_cents = inner.iter().map(|r| r.token_usage.estimated_cost_cents).sum();
        let avg_latency_ms = if total_calls > 0 {
            inner.iter().map(|r| r.latency_ms).sum::<u64>() as f64 / total_calls as f64
        } else {
            0.0
        };
        LlmAggregateStats {
            total_calls,
            successful_calls,
            failed_calls,
            total_input_tokens,
            total_output_tokens,
            total_cost_cents,
            avg_latency_ms,
        }
    }
}

fn classify_llm_error(err: &anyhow::Error) -> LlmErrorKind {
    let msg = err.to_string().to_lowercase();
    if msg.contains("401") || msg.contains("unauthorized") || msg.contains("auth") {
        return LlmErrorKind::Auth;
    }
    if msg.contains("429") || msg.contains("rate limit") || msg.contains("too many requests") {
        return LlmErrorKind::RateLimit;
    }
    if msg.contains("timeout") || msg.contains("timed out") || msg.contains("timedout") {
        return LlmErrorKind::Timeout;
    }
    if msg.contains("expected value")
        || msg.contains("invalid type")
        || msg.contains("parse error")
        || msg.contains("expected")
    {
        return LlmErrorKind::ParseError;
    }
    LlmErrorKind::Other(err.to_string())
}

#[tracing::instrument(skip_all)]
pub async fn record_llm_call(
    provider: &dyn LlmProvider,
    call_type: LlmCallType,
    system: &str,
    user: &str,
    history: Option<&LlmCallHistory>,
    event_tx: Option<&tokio::sync::mpsc::UnboundedSender<LlmCallRecord>>,
) -> Result<(String, TokenUsage)> {
    let start = Instant::now();
    let result = provider.complete(system, user).await;
    let latency_ms = start.elapsed().as_millis() as u64;

    let record = match &result {
        Ok((response, usage)) => {
            tracing::info!(
                llm.call_type = %call_type,
                llm.model = %provider.model_name(),
                llm.latency_ms = latency_ms,
                llm.input_tokens = usage.input_tokens,
                llm.output_tokens = usage.output_tokens,
                llm.cost_cents = usage.estimated_cost_cents,
                llm.status = "success",
                "LLM call completed",
            );
            LlmCallRecord {
                call_type,
                model: provider.model_name().to_string(),
                system_chars: system.len(),
                user_chars: user.len(),
                response_chars: response.len(),
                latency_ms,
                token_usage: usage.clone(),
                success: true,
                error_kind: None,
                error_message: None,
                timestamp: chrono::Utc::now().to_rfc3339(),
            }
        }
        Err(e) => {
            let error_kind = classify_llm_error(e);
            tracing::warn!(
                llm.call_type = %call_type,
                llm.model = %provider.model_name(),
                llm.latency_ms = latency_ms,
                llm.status = "error",
                llm.error_kind = %error_kind,
                "LLM call failed",
            );
            LlmCallRecord {
                call_type,
                model: provider.model_name().to_string(),
                system_chars: system.len(),
                user_chars: user.len(),
                response_chars: 0,
                latency_ms,
                token_usage: crate::config::TokenUsage::default(),
                success: false,
                error_kind: Some(error_kind),
                error_message: Some(e.to_string()),
                timestamp: chrono::Utc::now().to_rfc3339(),
            }
        }
    };

    if let Some(history) = history {
        history.push(record.clone());
    }
    if let Some(tx) = event_tx {
        let _ = tx.send(record);
    }

    result
}
