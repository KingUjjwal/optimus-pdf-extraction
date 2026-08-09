use crate::config::TokenUsage;
use crate::llm::LlmProvider;
use crate::observability::{record_llm_call, LlmCallHistory, LlmCallRecord, LlmCallType};
use crate::templates;
use anyhow::{anyhow, Result};
use optimus_core::TextSpan;
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
        format!(
            "{}\n\n{}",
            prompt,
            templates::schema_discovery_user(ascii_grid)
        )
    } else {
        templates::schema_discovery_user(ascii_grid)
    };
    let (response, usage) = record_llm_call(
        llm,
        LlmCallType::SchemaDiscovery,
        system,
        &user,
        history,
        event_tx,
    )
    .await?;

    let cleaned = response
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let parsed: serde_json::Value = serde_json::from_str(cleaned).map_err(|e| {
        anyhow!(
            "LLM schema response is not valid JSON: {}. Raw: {}",
            e,
            cleaned
        )
    })?;

    if parsed.as_object().is_none_or(|o| o.is_empty()) {
        return Err(anyhow!(
            "LLM returned empty schema object. Raw: {}",
            cleaned
        ));
    }

    Ok((parsed.to_string(), usage))
}

#[tracing::instrument(skip_all)]
pub fn discover_schema_offline(_ascii_grid: &str) -> String {
    FALLBACK_SCHEMA.to_string()
}

/// Heuristic schema inference from document text spans.
/// Finds label:value pairs (either split across spans or inline in one span),
/// infers types from sample values, returns JSON schema.
pub fn infer_schema(spans: &[TextSpan]) -> String {
    let quality = optimus_core::analyze_text_quality(spans);
    if quality.has_encoding_issues {
        tracing::warn!(
            "infer_schema: text-quality gate flagged {} page(s) as garbled: {:?}",
            quality.pages_needing_ocr.len(),
            quality.reasons_by_page,
        );
    }

    let mut fields: Vec<(String, String)> = Vec::new();
    let mut seen = HashSet::new();
    let tolerance = 14.0;

    for span in spans {
        let text = span.text.trim();
        let Some(colon_idx) = text.find(':') else {
            continue;
        };
        let label = text[..colon_idx].trim();
        if label.is_empty() || label.len() > 40 || !label.chars().any(|c| c.is_alphabetic()) {
            continue;
        }
        let inline_val = text[colon_idx + 1..].trim();
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
        let field_type = if !inline_val.is_empty() && inline_val.len() <= 80 {
            infer_field_type(inline_val)
        } else {
            spans
                .iter()
                .filter(|o| o.text.trim() != text && o.x0 >= span.x1)
                .filter(|o| {
                    let ca = (span.y0 + span.y1) / 2.0;
                    let cb = (o.y0 + o.y1) / 2.0;
                    (ca - cb).abs() <= tolerance
                })
                .min_by(|a, b| {
                    (a.x0 - span.x1)
                        .partial_cmp(&(b.x0 - span.x1))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|v| infer_field_type(v.text.trim()))
                .unwrap_or("string")
        };
        fields.push((field_name, field_type.to_string()));
    }

    if fields.is_empty() {
        return FALLBACK_SCHEMA.to_string();
    }
    let obj: serde_json::Value = fields
        .into_iter()
        .map(|(k, v)| (k, serde_json::Value::String(v)))
        .collect();
    serde_json::to_string(&obj).unwrap_or_else(|_| FALLBACK_SCHEMA.to_string())
}

/// Detects transaction-table columns from document spans, returning (label, output key)
/// pairs. A transaction table is recognized when at least 3 date-like cells (DD-MMM-YYYY)
/// share a column, and a header row with recognized column keywords sits above them.
pub fn detect_transactions_columns(spans: &[TextSpan]) -> Vec<(String, String)> {
    fn is_date_cell(t: &str) -> bool {
        let parts: Vec<&str> = t.trim().split('-').collect();
        if parts.len() != 3 {
            return false;
        }
        let (d, m, y) = (parts[0], parts[1], parts[2]);
        d.len() <= 2
            && !d.is_empty()
            && d.chars().all(|c| c.is_numeric())
            && m.len() == 3
            && m.chars().all(|c| c.is_alphabetic())
            && y.len() == 4
            && y.chars().all(|c| c.is_numeric())
    }

    let date_rows: Vec<&TextSpan> = spans.iter().filter(|s| is_date_cell(&s.text)).collect();
    if date_rows.len() < 3 {
        return Vec::new();
    }
    let x0 = date_rows[0].x0;
    let aligned: Vec<&TextSpan> = date_rows
        .iter()
        .filter(|s| (s.x0 - x0).abs() < 20.0)
        .copied()
        .collect();
    if aligned.len() < 3 {
        return Vec::new();
    }

    // Topmost date cell marks the first table row (y grows upward); scan spans
    // above it from closest-to-table upward, keeping recognized header keywords.
    let first_row_y = aligned.iter().map(|s| s.y0).fold(f32::MIN, f32::max);
    let mut candidates: Vec<&TextSpan> = spans
        .iter()
        .filter(|s| s.y0 > first_row_y && s.y0 <= first_row_y + 40.0 && s.x0 >= 20.0)
        .collect();
    candidates.sort_by(|a, b| a.y0.partial_cmp(&b.y0).unwrap_or(std::cmp::Ordering::Equal));

    let mut headers: Vec<(String, String)> = Vec::new();
    let mut used = HashSet::new();
    for s in candidates {
        let label = s.text.trim();
        if label.is_empty() {
            continue;
        }
        let key = header_key(label);
        let known = matches!(
            key.as_str(),
            "date"
                | "narration"
                | "amount"
                | "units"
                | "price"
                | "balance"
                | "withdrawal"
                | "deposit"
                | "chq_ref"
        );
        if !known || !used.insert(key.clone()) {
            continue;
        }
        headers.push((label.to_string(), key));
    }

    if headers.len() < 3 || !headers.iter().any(|(_, k)| k == "date") {
        return Vec::new();
    }
    headers
}

/// Maps a table header label to a normalized snake_case output key.
pub fn header_key(label: &str) -> String {
    let l = label
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if let Some(k) = match l.as_str() {
        "date" => Some("date"),
        "transaction" | "narration" | "description" | "particulars" => Some("narration"),
        "amount" | "amount (inr)" => Some("amount"),
        "units" => Some("units"),
        "price" | "price per unit" | "price unit" | "nav price" => Some("price"),
        "balance" | "balance (inr)" | "closing balance" => Some("balance"),
        "withdrawal" | "debit" | "withdrawn" | "paid out" => Some("withdrawal"),
        "deposit" | "credit" | "paid in" => Some("deposit"),
        "chq" | "cheque" | "ref" | "reference" | "chq/ref" => Some("chq_ref"),
        _ => None,
    } {
        return k.to_string();
    }
    l.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

fn infer_field_type(sample: &str) -> &str {
    let s = sample.trim();
    if s.is_empty() || s.len() > 100 {
        return "string";
    }
    // ISO date: YYYY-MM-DD with valid ranges
    if s.len() == 10 && s.chars().filter(|c| *c == '-').count() == 2 {
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
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_numeric()))
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
            && rest.chars().all(|c| c.is_numeric() || c == '.' || c == ',')
            && rest.chars().filter(|c| *c == '.').count() <= 1
        {
            return "number";
        }
    }
    // Percentage: handles "99.9%", "1,000%", "50%" etc.
    if s.ends_with('%') && s.len() > 1 {
        let body = &s[..s.len() - 1];
        if !body.is_empty()
            && body.chars().all(|c| c.is_numeric() || c == '.' || c == ',')
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
            TextSpan {
                text: "INVOICE".into(),
                x0: 50.0,
                y0: 750.0,
                x1: 150.0,
                y1: 770.0,
                page: None,
                font_size: 0.0,
                is_bold: false,
                is_italic: false,
            },
            TextSpan {
                text: "Invoice Number:".into(),
                x0: 50.0,
                y0: 700.0,
                x1: 160.0,
                y1: 715.0,
                page: None,
                font_size: 0.0,
                is_bold: false,
                is_italic: false,
            },
            TextSpan {
                text: "INV-2026-001".into(),
                x0: 180.0,
                y0: 700.0,
                x1: 280.0,
                y1: 715.0,
                page: None,
                font_size: 0.0,
                is_bold: false,
                is_italic: false,
            },
            TextSpan {
                text: "Date:".into(),
                x0: 50.0,
                y0: 680.0,
                x1: 100.0,
                y1: 695.0,
                page: None,
                font_size: 0.0,
                is_bold: false,
                is_italic: false,
            },
            TextSpan {
                text: "2026-05-23".into(),
                x0: 180.0,
                y0: 680.0,
                x1: 270.0,
                y1: 695.0,
                page: None,
                font_size: 0.0,
                is_bold: false,
                is_italic: false,
            },
            TextSpan {
                text: "Bill To:".into(),
                x0: 50.0,
                y0: 630.0,
                x1: 100.0,
                y1: 645.0,
                page: None,
                font_size: 0.0,
                is_bold: false,
                is_italic: false,
            },
            TextSpan {
                text: "Acme Corp".into(),
                x0: 50.0,
                y0: 610.0,
                x1: 120.0,
                y1: 625.0,
                page: None,
                font_size: 0.0,
                is_bold: false,
                is_italic: false,
            },
            TextSpan {
                text: "Total:".into(),
                x0: 400.0,
                y0: 400.0,
                x1: 450.0,
                y1: 415.0,
                page: None,
                font_size: 0.0,
                is_bold: false,
                is_italic: false,
            },
            TextSpan {
                text: "$500.50".into(),
                x0: 500.0,
                y0: 400.0,
                x1: 555.0,
                y1: 415.0,
                page: None,
                font_size: 0.0,
                is_bold: false,
                is_italic: false,
            },
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

    #[test]
    fn test_detect_transactions_columns() {
        let mut spans = vec![
            TextSpan {
                text: "Date".into(),
                x0: 28.0,
                y0: 500.0,
                x1: 65.0,
                y1: 509.0,
                page: None,
                font_size: 0.0,
                is_bold: false,
                is_italic: false,
            },
            TextSpan {
                text: "Transaction".into(),
                x0: 74.0,
                y0: 500.0,
                x1: 300.0,
                y1: 509.0,
                page: None,
                font_size: 0.0,
                is_bold: false,
                is_italic: false,
            },
            TextSpan {
                text: "Amount".into(),
                x0: 340.0,
                y0: 500.0,
                x1: 372.0,
                y1: 509.0,
                page: None,
                font_size: 0.0,
                is_bold: false,
                is_italic: false,
            },
            TextSpan {
                text: "Units".into(),
                x0: 402.0,
                y0: 500.0,
                x1: 430.0,
                y1: 509.0,
                page: None,
                font_size: 0.0,
                is_bold: false,
                is_italic: false,
            },
            TextSpan {
                text: "Price".into(),
                x0: 461.0,
                y0: 500.0,
                x1: 488.0,
                y1: 509.0,
                page: None,
                font_size: 0.0,
                is_bold: false,
                is_italic: false,
            },
            TextSpan {
                text: "Balance".into(),
                x0: 538.0,
                y0: 500.0,
                x1: 566.0,
                y1: 509.0,
                page: None,
                font_size: 0.0,
                is_bold: false,
                is_italic: false,
            },
        ];
        for (i, y) in [480.0, 470.0, 460.0].iter().enumerate() {
            let date = format!("0{}-Jan-202{}", i + 1, 5 - i);
            spans.push(TextSpan {
                text: date,
                x0: 28.0,
                y0: *y,
                x1: 65.0,
                y1: y + 9.0,
                page: None,
                font_size: 0.0,
                is_bold: false,
                is_italic: false,
            });
            spans.push(TextSpan {
                text: "SIP Purchase".into(),
                x0: 74.0,
                y0: *y,
                x1: 300.0,
                y1: y + 9.0,
                page: None,
                font_size: 0.0,
                is_bold: false,
                is_italic: false,
            });
            spans.push(TextSpan {
                text: "7,999.60".into(),
                x0: 340.0,
                y0: *y,
                x1: 372.0,
                y1: y + 9.0,
                page: None,
                font_size: 0.0,
                is_bold: false,
                is_italic: false,
            });
            spans.push(TextSpan {
                text: "67.647".into(),
                x0: 402.0,
                y0: *y,
                x1: 430.0,
                y1: y + 9.0,
                page: None,
                font_size: 0.0,
                is_bold: false,
                is_italic: false,
            });
            spans.push(TextSpan {
                text: "118.2548".into(),
                x0: 461.0,
                y0: *y,
                x1: 488.0,
                y1: y + 9.0,
                page: None,
                font_size: 0.0,
                is_bold: false,
                is_italic: false,
            });
            spans.push(TextSpan {
                text: "2,285.727".into(),
                x0: 538.0,
                y0: *y,
                x1: 566.0,
                y1: y + 9.0,
                page: None,
                font_size: 0.0,
                is_bold: false,
                is_italic: false,
            });
        }

        let cols = detect_transactions_columns(&spans);
        assert!(cols.iter().any(|(l, k)| l == "Date" && k == "date"));
        assert!(cols
            .iter()
            .any(|(l, k)| l == "Transaction" && k == "narration"));
        assert!(cols.iter().any(|(l, k)| l == "Amount" && k == "amount"));
        assert!(cols.iter().any(|(l, k)| l == "Balance" && k == "balance"));
        assert!(cols.len() >= 3);
    }
}
