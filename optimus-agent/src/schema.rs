use crate::config::TokenUsage;
use crate::llm::LlmProvider;
use crate::templates;
use crate::observability::{LlmCallRecord, LlmCallType, LlmCallHistory, record_llm_call};
use optimus_core::TextSpan;
use anyhow::{Result, anyhow};
use std::collections::HashSet;

const FALLBACK_SCHEMA: &str = r#"{"invoice_number":"string","date":"string","total":"string"}"#;

pub async fn discover_schema_llm(
    ascii_grid: &str,
    provider: Option<&dyn LlmProvider>,
    custom_prompt: Option<&str>,
    history: Option<&LlmCallHistory>,
    event_tx: Option<&tokio::sync::mpsc::UnboundedSender<LlmCallRecord>>,
) -> Result<(String, TokenUsage)> {
    let Some(llm) = provider else {
        return Ok((FALLBACK_SCHEMA.to_string(), TokenUsage::default()));
    };

    let system = templates::SCHEMA_DISCOVERY_SYSTEM;
    let user = if let Some(prompt) = custom_prompt {
        format!("{}\n\n{}", prompt, templates::schema_discovery_user(ascii_grid))
    } else {
        templates::schema_discovery_user(ascii_grid)
    };
    let (response, usage) = record_llm_call(llm, LlmCallType::SchemaDiscovery, system, &user, history, event_tx).await?;

    let cleaned = response.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();

    let parsed: serde_json::Value = serde_json::from_str(cleaned)
        .map_err(|e| anyhow!("LLM schema response is not valid JSON: {}. Raw: {}", e, cleaned))?;

    if parsed.as_object().map_or(true, |o| o.is_empty()) {
        return Err(anyhow!("LLM returned empty schema object. Raw: {}", cleaned));
    }

    Ok((parsed.to_string(), usage))
}

#[tracing::instrument(skip_all)]
pub fn discover_schema_offline(_ascii_grid: &str) -> String {
    FALLBACK_SCHEMA.to_string()
}

/// Heuristic schema inference from document text spans.
/// Finds label:value pairs via spatial right-neighbor search,
/// infers types from sample values, returns JSON schema.
pub fn infer_schema(spans: &[TextSpan]) -> String {
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut seen = HashSet::new();
    let tolerance = 10.0;

    for span in spans {
        let text = span.text.trim();
        if !text.ends_with(':') {
            continue;
        }
        let label = text.trim_end_matches(':').trim();
        if label.is_empty() {
            continue;
        }
        let field_name: String = label
            .to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == ' ')
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("_");
        if !seen.insert(field_name.clone()) {
            continue;
        }
        let field_type = spans
            .iter()
            .filter(|o| o.text.trim() != text && o.x0 >= span.x1)
            .filter(|o| {
                let ca = (span.y0 + span.y1) / 2.0;
                let cb = (o.y0 + o.y1) / 2.0;
                (ca - cb).abs() <= tolerance
            })
            .min_by(|a, b| (a.x0 - span.x1).partial_cmp(&(b.x0 - span.x1)).unwrap_or(std::cmp::Ordering::Equal))
            .map(|v| infer_field_type(v.text.trim()))
            .unwrap_or("string");
        fields.push((field_name, field_type.to_string()));
    }

    if fields.is_empty() {
        return FALLBACK_SCHEMA.to_string();
    }
    let obj: serde_json::Value = fields.into_iter().map(|(k, v)| (k, serde_json::Value::String(v))).collect();
    serde_json::to_string(&obj).unwrap_or_else(|_| FALLBACK_SCHEMA.to_string())
}

fn infer_field_type(sample: &str) -> &str {
    let s = sample.trim();
    if s.is_empty() || s.len() > 100 {
        return "string";
    }
    // ISO date: YYYY-MM-DD with valid ranges
    if s.len() == 10
        && s.chars().filter(|c| *c == '-').count() == 2
    {
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() == 3
            && parts.iter().all(|p| p.len() == 4 || p.len() == 2)
            && parts.iter().all(|p| p.chars().all(|c| c.is_numeric()))
        {
            if let (Ok(y), Ok(m), Ok(d)) = (
                parts[0].parse::<u16>(),
                parts[1].parse::<u8>(),
                parts[2].parse::<u8>(),
            ) {
                if (2020..=2099).contains(&y) && (1..=12).contains(&m) && (1..=31).contains(&d) {
                    return "date";
                }
            }
        }
    }
    // US date: M[M]/D[D]/YYYY or MM/DD/YYYY with valid ranges
    if s.chars().filter(|c| *c == '/').count() == 2 {
        let parts: Vec<&str> = s.split('/').collect();
        if parts.len() == 3
            && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_numeric()))
        {
            if let (Ok(m), Ok(d), Ok(y)) = (
                parts[0].parse::<u8>(),
                parts[1].parse::<u8>(),
                parts[2].parse::<u16>(),
            ) {
                if (1..=12).contains(&m) && (1..=31).contains(&d) && (2020..=2099).contains(&y) {
                    return "date";
                }
            }
        }
    }
    // Currency: $X.XX (also handles thousand-separated like $1,000.50)
    if s.starts_with('$') && s.len() > 1 {
        let rest = &s[1..];
        if !rest.is_empty()
            && rest
                .chars()
                .all(|c| c.is_numeric() || c == '.' || c == ',')
            && rest.chars().filter(|c| *c == '.').count() <= 1
        {
            return "number";
        }
    }
    // Percentage: handles "99.9%", "1,000%", "50%" etc.
    if s.ends_with('%') && s.len() > 1 {
        let body = &s[..s.len() - 1];
        if !body.is_empty()
            && body
                .chars()
                .all(|c| c.is_numeric() || c == '.' || c == ',')
            && body.chars().filter(|c| *c == '.').count() <= 1
        {
            return "number";
        }
    }
    // Number: digits, at most 2 non-digit chars (decimal, comma, minus)
    let digits: String = s.chars().filter(|c| c.is_numeric()).collect();
    if !digits.is_empty() {
        let non_digit = s.chars().filter(|c| !c.is_numeric()).count();
        if non_digit <= 3
            && !s.chars().any(|c| c.is_alphabetic())
            && s.chars().filter(|c| *c == '-').count() <= 1
            && s.chars().filter(|c| *c == '.').count() <= 1
        {
            return "number";
        }
    }
    "string"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_spans() -> Vec<TextSpan> {
        vec![
            TextSpan { text: "INVOICE".into(), x0: 50.0, y0: 750.0, x1: 150.0, y1: 770.0 },
            TextSpan { text: "Invoice Number:".into(), x0: 50.0, y0: 700.0, x1: 160.0, y1: 715.0 },
            TextSpan { text: "INV-2026-001".into(), x0: 180.0, y0: 700.0, x1: 280.0, y1: 715.0 },
            TextSpan { text: "Date:".into(), x0: 50.0, y0: 680.0, x1: 100.0, y1: 695.0 },
            TextSpan { text: "2026-05-23".into(), x0: 180.0, y0: 680.0, x1: 270.0, y1: 695.0 },
            TextSpan { text: "Bill To:".into(), x0: 50.0, y0: 630.0, x1: 100.0, y1: 645.0 },
            TextSpan { text: "Acme Corp".into(), x0: 50.0, y0: 610.0, x1: 120.0, y1: 625.0 },
            TextSpan { text: "Total:".into(), x0: 400.0, y0: 400.0, x1: 450.0, y1: 415.0 },
            TextSpan { text: "$500.50".into(), x0: 500.0, y0: 400.0, x1: 555.0, y1: 415.0 },
        ]
    }

    #[test]
    fn test_infer_schema() {
        let spans = mock_spans();
        let schema = infer_schema(&spans);
        let parsed: serde_json::Value = serde_json::from_str(&schema).unwrap();
        let obj = parsed.as_object().unwrap();
        assert!(obj.contains_key("invoice_number"));
        assert!(obj.contains_key("date"));
        assert!(obj.contains_key("total"));
        assert_eq!(obj["invoice_number"], "string");
        assert_eq!(obj["date"], "date");
        assert_eq!(obj["total"], "number");
    }

    #[test]
    fn test_infer_field_type() {
        assert_eq!(infer_field_type("2026-05-23"), "date");
        assert_eq!(infer_field_type("01/15/2026"), "date");
        assert_eq!(infer_field_type("$500.50"), "number");
        assert_eq!(infer_field_type("42"), "number");
        assert_eq!(infer_field_type("99.9%"), "number");
        assert_eq!(infer_field_type("Acme Corp"), "string");
        assert_eq!(infer_field_type(""), "string");
    }
}
