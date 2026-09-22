use crate::config::{LlmConfig, TokenUsage};
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Transient HTTP statuses worth retrying (rate limit + server errors).
fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 429 | 500 | 502 | 503 | 504)
}

/// Exponential backoff: 500ms, 1s, 2s, ... capped at 8s.
fn retry_backoff(attempt: u32) -> Duration {
    let ms = 500u64.saturating_mul(1u64 << attempt.saturating_sub(1).min(4));
    Duration::from_millis(ms.min(8_000))
}

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Debug, Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
    usage: Option<UsageInfo>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChatChoiceMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct UsageInfo {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
}

#[derive(Debug, Serialize)]
struct AnthropicRequest<'a> {
    model: &'a str,
    system: &'a str,
    messages: Vec<AnthropicMessage<'a>>,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Debug, Serialize)]
struct AnthropicMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicContent {
    text: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: Option<u32>,
    output_tokens: Option<u32>,
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, system: &str, user: &str) -> Result<(String, TokenUsage)>;
    fn model_name(&self) -> &str;
    fn cost_config(&self) -> (f64, f64);
}

struct ProviderBase {
    api_key: String,
    base_url: String,
    model: String,
    max_tokens: u32,
    client: reqwest::Client,
    input_cost_per_1m: f64,
    output_cost_per_1m: f64,
}

impl ProviderBase {
    fn new(config: &LlmConfig) -> Self {
        // A hung provider must not stall the compile loop forever: bound the
        // whole request and the connect phase.
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(90))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            api_key: config.api_key.clone().unwrap_or_default(),
            base_url: config.base_url.trim_end_matches('/').to_string(),
            model: config.model.clone(),
            max_tokens: config.max_tokens_per_call,
            client,
            input_cost_per_1m: config.input_cost_per_1m,
            output_cost_per_1m: config.output_cost_per_1m,
        }
    }

    /// Sends a request built by `make`, retrying transient failures (429/5xx,
    /// timeouts, connection resets) with exponential backoff.
    async fn send_with_retry(
        &self,
        make: impl Fn() -> reqwest::RequestBuilder,
        max_attempts: u32,
    ) -> Result<reqwest::Response> {
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            match make().send().await {
                Ok(resp) => {
                    if resp.status().is_success() || !is_retryable_status(resp.status()) {
                        return Ok(resp);
                    }
                    if attempt >= max_attempts {
                        return Ok(resp);
                    }
                    let delay = retry_backoff(attempt);
                    tracing::warn!(
                        "LLM request got {} (attempt {}/{}), retrying in {:?}",
                        resp.status(),
                        attempt,
                        max_attempts,
                        delay,
                    );
                    tokio::time::sleep(delay).await;
                }
                Err(e) => {
                    let transient = e.is_timeout() || e.is_connect() || e.is_request();
                    if !transient || attempt >= max_attempts {
                        return Err(e.into());
                    }
                    let delay = retry_backoff(attempt);
                    tracing::warn!(
                        "LLM request failed (attempt {}/{}): {} — retrying in {:?}",
                        attempt,
                        max_attempts,
                        e,
                        delay,
                    );
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    fn estimate_tokens(&self, text: &str) -> usize {
        let chars = text.chars().count();
        let cjk = text.chars().filter(|c| c > &'\u{2E80}').count();
        let ascii = chars - cjk;
        (ascii / 4) + (cjk / 2)
    }

    fn compute_cost(&self, input_tokens: usize, output_tokens: usize) -> u32 {
        let input_cost = (input_tokens as f64 / 1_000_000.0) * self.input_cost_per_1m;
        let output_cost = (output_tokens as f64 / 1_000_000.0) * self.output_cost_per_1m;
        ((input_cost + output_cost) * 100.0).round() as u32
    }

    fn fallback_usage(&self, system: &str, user: &str, response: &str) -> TokenUsage {
        let input_tokens = self.estimate_tokens(system) + self.estimate_tokens(user);
        let output_tokens = self.estimate_tokens(response);
        TokenUsage {
            input_tokens,
            output_tokens,
            estimated_cost_cents: self.compute_cost(input_tokens, output_tokens),
        }
    }
}

/// OpenAI-compatible chat completion provider.
pub struct ChatProvider {
    base: ProviderBase,
}

impl ChatProvider {
    #[tracing::instrument(skip_all)]
    pub fn new(config: &LlmConfig) -> Self {
        Self {
            base: ProviderBase::new(config),
        }
    }
}

#[async_trait]
impl LlmProvider for ChatProvider {
    #[tracing::instrument(skip(self, system, user), fields(model = %self.base.model))]
    async fn complete(&self, system: &str, user: &str) -> Result<(String, TokenUsage)> {
        let endpoint = format!("{}/chat/completions", self.base.base_url);
        tracing::info!(
            "LLM request → {} | system={} chars | user={} chars | max_tokens={}",
            self.base.model,
            system.len(),
            user.len(),
            self.base.max_tokens,
        );
        tracing::debug!("LLM system prompt:\n{}", system);
        tracing::debug!("LLM user prompt:\n{}", user);

        let messages = vec![
            ChatMessage {
                role: "system",
                content: system,
            },
            ChatMessage {
                role: "user",
                content: user,
            },
        ];

        let body = ChatRequest {
            model: &self.base.model,
            messages,
            max_tokens: self.base.max_tokens,
            temperature: 0.1,
        };

        let response = self
            .base
            .send_with_retry(
                || {
                    self.base
                        .client
                        .post(&endpoint)
                        .header("Authorization", format!("Bearer {}", self.base.api_key))
                        .header("Content-Type", "application/json")
                        .json(&body)
                },
                3,
            )
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            tracing::error!("LLM API error {} from {}: {}", status, endpoint, text);
            return Err(anyhow::anyhow!(
                "LLM API error {} from {}: {}",
                status,
                endpoint,
                text
            ));
        }

        let resp: ChatResponse = response.json().await?;
        let content = resp
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .unwrap_or_default();

        let usage = if let Some(u) = resp.usage {
            let input_tokens = u.prompt_tokens.unwrap_or(0) as usize;
            let output_tokens = u.completion_tokens.unwrap_or(0) as usize;
            TokenUsage {
                input_tokens,
                output_tokens,
                estimated_cost_cents: self.base.compute_cost(input_tokens, output_tokens),
            }
        } else {
            self.base.fallback_usage(system, user, &content)
        };

        tracing::info!(
            "LLM response ← {} | {} chars | in={} tok | out={} tok | ~${:.4}",
            self.base.model,
            content.len(),
            usage.input_tokens,
            usage.output_tokens,
            usage.estimated_cost_cents as f64 / 100.0,
        );
        tracing::debug!("LLM response body:\n{}", content);

        Ok((content, usage))
    }

    fn model_name(&self) -> &str {
        &self.base.model
    }
    fn cost_config(&self) -> (f64, f64) {
        (self.base.input_cost_per_1m, self.base.output_cost_per_1m)
    }
}

/// Anthropic Claude API provider.
pub struct AnthropicProvider {
    base: ProviderBase,
}

impl AnthropicProvider {
    #[tracing::instrument(skip_all)]
    pub fn new(config: &LlmConfig) -> Self {
        Self {
            base: ProviderBase::new(config),
        }
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    #[tracing::instrument(skip(self, system, user), fields(model = %self.base.model))]
    async fn complete(&self, system: &str, user: &str) -> Result<(String, TokenUsage)> {
        let endpoint = format!("{}/messages", self.base.base_url);
        tracing::info!(
            "LLM request → {} | system={} chars | user={} chars | max_tokens={}",
            self.base.model,
            system.len(),
            user.len(),
            self.base.max_tokens,
        );
        tracing::debug!("LLM system prompt:\n{}", system);
        tracing::debug!("LLM user prompt:\n{}", user);

        let messages = vec![AnthropicMessage {
            role: "user",
            content: user,
        }];

        let body = AnthropicRequest {
            model: &self.base.model,
            system,
            messages,
            max_tokens: self.base.max_tokens,
            temperature: 0.1,
        };

        let response = self
            .base
            .send_with_retry(
                || {
                    self.base
                        .client
                        .post(&endpoint)
                        .header("x-api-key", &self.base.api_key)
                        .header("anthropic-version", "2023-06-01")
                        .header("Content-Type", "application/json")
                        .json(&body)
                },
                3,
            )
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            tracing::error!("Anthropic API error {} from {}: {}", status, endpoint, text);
            return Err(anyhow::anyhow!(
                "Anthropic API error {} from {}: {}",
                status,
                endpoint,
                text
            ));
        }

        let resp: AnthropicResponse = response.json().await?;
        let content = resp
            .content
            .into_iter()
            .next()
            .map(|c| c.text)
            .unwrap_or_default();

        let usage = if let Some(u) = resp.usage {
            let input_tokens = u.input_tokens.unwrap_or(0) as usize;
            let output_tokens = u.output_tokens.unwrap_or(0) as usize;
            TokenUsage {
                input_tokens,
                output_tokens,
                estimated_cost_cents: self.base.compute_cost(input_tokens, output_tokens),
            }
        } else {
            self.base.fallback_usage(system, user, &content)
        };

        tracing::info!(
            "LLM response ← {} | {} chars | in={} tok | out={} tok | ~${:.4}",
            self.base.model,
            content.len(),
            usage.input_tokens,
            usage.output_tokens,
            usage.estimated_cost_cents as f64 / 100.0,
        );
        tracing::debug!("LLM response body:\n{}", content);

        Ok((content, usage))
    }

    fn model_name(&self) -> &str {
        &self.base.model
    }
    fn cost_config(&self) -> (f64, f64) {
        (self.base.input_cost_per_1m, self.base.output_cost_per_1m)
    }
}

#[tracing::instrument(skip_all)]
pub fn create_provider(config: &LlmConfig) -> Option<Box<dyn LlmProvider>> {
    if config.provider_enabled {
        if config.base_url.contains("anthropic.com") {
            Some(Box::new(AnthropicProvider::new(config)))
        } else {
            Some(Box::new(ChatProvider::new(config)))
        }
    } else {
        None
    }
}
