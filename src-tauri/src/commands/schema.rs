use optimus_core::{TextSpan, generate_ascii_grid};
use optimus_agent::{discover_schema_llm, infer_schema, TokenUsage};
use std::time::Duration;
use tauri::AppHandle;

use super::emit_event;
use super::types::SchemaInferenceResult;
use crate::llm_util::get_llm_provider;

#[tauri::command]
#[tracing::instrument(level = "info", skip(app))]
pub async fn discover_schema_command(spans_json: String, custom_prompt: Option<String>, app: AppHandle) -> Result<String, String> {
    let app_clone = app.clone();
    let (spans, grid) = tokio::task::spawn_blocking(move || {
        let spans: Vec<TextSpan> = serde_json::from_str(&spans_json)
            .map_err(|e| format!("Failed to parse spans JSON: {}", e))?;
        let grid = generate_ascii_grid(&spans);
        Ok::<(Vec<TextSpan>, String), String>((spans, grid))
    }).await.map_err(|e| format!("Task error: {}", e))??;

    tracing::info!("Parsed {} spans, grid size: {} chars", spans.len(), grid.len());
    if custom_prompt.is_some() {
        tracing::info!("Using custom prompt for schema inference");
    }

    let provider = get_llm_provider();
    let schema = if let Some(provider) = provider {
        tracing::info!("Using LLM provider {} for schema inference", provider.model_name());

        match tokio::time::timeout(
            Duration::from_secs(30),
            discover_schema_llm(&grid, Some(provider.as_ref()), custom_prompt.as_deref(), None, None)
        ).await {
            Ok(Ok((s, usage))) => {
                tracing::info!("LLM schema inference succeeded: {} chars, input={}, output={}", s.len(), usage.input_tokens, usage.output_tokens);
                emit_event(&app_clone, "pipeline:schema-inferred", serde_json::json!({
                    "schema": &s,
                    "provider": provider.model_name(),
                    "fallback": false
                }));
                s
            }
            Ok(Err(e)) => {
                tracing::warn!("LLM schema inference failed: {}, falling back to heuristic", e);
                let fallback = infer_schema(&spans);
                tracing::info!("Heuristic schema: {} chars", fallback.len());
                emit_event(&app_clone, "pipeline:schema-inferred", serde_json::json!({
                    "schema": &fallback,
                    "provider": "heuristic",
                    "fallback": true
                }));
                fallback
            }
            Err(_) => {
                tracing::warn!("LLM schema inference timed out, falling back to heuristic");
                let fallback = infer_schema(&spans);
                tracing::info!("Heuristic schema (timeout): {} chars", fallback.len());
                emit_event(&app_clone, "pipeline:schema-inferred", serde_json::json!({
                    "schema": &fallback,
                    "provider": "heuristic",
                    "fallback": true
                }));
                fallback
            }
        }
    } else {
        tracing::info!("No LLM provider, using heuristic schema inference");
        let fallback = infer_schema(&spans);
        tracing::info!("Heuristic schema: {} chars", fallback.len());
        emit_event(&app_clone, "pipeline:schema-inferred", serde_json::json!({
            "schema": &fallback,
            "provider": "heuristic",
            "fallback": false
        }));
        fallback
    };

    Ok(schema)
}

#[tauri::command]
#[tracing::instrument(level = "info", skip(app))]
pub async fn infer_schema_llm_command(
    grid: String,
    spans_json: String,
    app: AppHandle,
) -> Result<SchemaInferenceResult, String> {
    let provider = get_llm_provider();
    let provider_name = provider.as_ref().map(|p| p.model_name().to_string()).unwrap_or_else(|| "offline".into());

    if let Some(llm) = provider {
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            discover_schema_llm(&grid, Some(llm.as_ref()), None, None, None),
        ).await;

        match result {
            Ok(Ok((schema, usage))) => {
                emit_event(&app, "pipeline:schema-inferred", serde_json::json!({
                    "schema": &schema,
                    "provider": &provider_name,
                    "token_usage": &usage,
                    "fallback": false,
                }));
                return Ok(SchemaInferenceResult {
                    schema,
                    provider_used: provider_name,
                    token_usage: usage,
                    fallback: false,
                });
            }
            Ok(Err(e)) => {
                emit_event(&app, "pipeline:schema-inferred", serde_json::json!({
                    "error": e.to_string(),
                    "provider": &provider_name,
                    "fallback": true,
                }));
                // LLM failed, fall through to heuristic
            }
            Err(_) => {
                emit_event(&app, "pipeline:schema-inferred", serde_json::json!({
                    "error": "LLM timeout after 30s",
                    "provider": &provider_name,
                    "fallback": true,
                }));
                // Timeout, fall through to heuristic
            }
        }
    }

    // Fallback: heuristic schema from spans
    let spans: Vec<TextSpan> = serde_json::from_str(&spans_json).unwrap_or_default();
    let schema = if !spans.is_empty() {
        infer_schema(&spans)
    } else {
        serde_json::json!({"invoice_number":"string","date":"string","total":"string"}).to_string()
    };

    emit_event(&app, "pipeline:schema-inferred", serde_json::json!({
        "schema": &schema,
        "provider": "heuristic",
        "fallback": true,
    }));

    Ok(SchemaInferenceResult {
        schema,
        provider_used: "heuristic".into(),
        token_usage: TokenUsage::default(),
        fallback: true,
    })
}
