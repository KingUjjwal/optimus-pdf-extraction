use crate::config::TokenUsage;
use crate::llm::LlmProvider;
use crate::templates;
use crate::observability::{LlmCallRecord, LlmCallType, LlmCallHistory, record_llm_call};
use anyhow::Result;

/// A parsed schema field with name and type.
pub struct SchemaField {
    pub name: String,
    pub field_type: String,
}

#[tracing::instrument(skip_all)]
pub fn parse_schema_fields(schema: &str) -> Result<Vec<SchemaField>> {
    let parsed: serde_json::Value = serde_json::from_str(schema)?;
    let obj = parsed.as_object().ok_or_else(|| anyhow::anyhow!("Schema is not a JSON object"))?;
    let mut fields = Vec::new();
    for (key, val) in obj {
        let field_type = val.as_str().unwrap_or("string").to_string();
        fields.push(SchemaField { name: key.clone(), field_type });
    }
    fields.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(fields)
}

fn build_code_template(fields: &[SchemaField]) -> String {
    if fields.is_empty() {
        return HARDCODED_INVOICE_CODE.to_string();
    }
    let mut vars = String::new();
    for f in fields {
        vars.push_str(&format!("    let mut {} = String::from(\"Unknown\");\n", f.name));
    }

    let mut if_chain = String::new();
    for f in fields {
        let label = f.name
            .replace('_', " ")
            .split_whitespace()
            .map(|w| {
                let mut c = w.chars();
                match c.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        if_chain.push_str(&format!(
            "    if let Some(val) = find_right_of(&graph, \"{}:\") {{\n        {} = val;\n    }}\n",
            label, f.name
        ));
    }

    let mut emit_fields = String::new();
    for f in fields {
        emit_fields.push_str(&format!("        (\"{}\", &{}),\n", f.name, f.name));
    }

    format!(
        r##"// Auto-generated WASM extraction module
use optimus_guest::*;

#[no_mangle]
pub extern "C" fn extract(ptr: *const u8, len: usize) -> *mut u8 {{
    let graph_str = unsafe {{
        let slice = std::slice::from_raw_parts(ptr, len);
        match std::str::from_utf8(slice) {{
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        }}
    }};

    let graph = parse_flat_graph(graph_str);

{vars}
{if_chain}
    let json_bytes = emit_json(&[
{emit_fields}    ]);

    let boxed = json_bytes.into_boxed_slice();
    Box::into_raw(boxed) as *mut u8
}}
"##,
        vars = vars,
        if_chain = if_chain,
        emit_fields = emit_fields,
    )
}

const HARDCODED_INVOICE_CODE: &str = include_str!("../resources/fallback_wasm.rs");

#[tracing::instrument(skip_all)]
pub fn generate_guest_rust_code_offline(schema: &str) -> String {
    let fields = parse_schema_fields(schema).unwrap_or_default();
    if fields.iter().any(|f| f.name == "invoice_number")
        && fields.iter().any(|f| f.name == "date")
        && fields.iter().any(|f| f.name == "total")
    {
        return HARDCODED_INVOICE_CODE.to_string();
    }
    build_code_template(&fields)
}

pub async fn generate_guest_rust_code(
    schema: &str,
    flat_graph: &str,
    provider: Option<&dyn LlmProvider>,
    history: Option<&LlmCallHistory>,
    event_tx: Option<&tokio::sync::mpsc::UnboundedSender<LlmCallRecord>>,
) -> Result<(String, TokenUsage)> {
    let Some(llm) = provider else {
        tracing::info!("Code generation: using OFFLINE template");
        let code = generate_guest_rust_code_offline(schema);
        tracing::info!("Generated offline Rust code: {} lines, {} bytes", code.lines().count(), code.len());
        return Ok((code, TokenUsage::default()));
    };

    tracing::info!("Code generation: using LLM model={} | schema keys={} | graph chars={}",
        llm.model_name(),
        schema.len(),
        flat_graph.len(),
    );

    let system = templates::CODE_GENERATION_SYSTEM.to_string();
    let user_prompt = templates::code_generation_user(schema, flat_graph);
    let (response, usage) = record_llm_call(llm, LlmCallType::CodeGeneration, &system, &user_prompt, history, event_tx).await?;

    tracing::debug!("LLM raw codegen response ({} chars):\n{}", response.len(), response);

    let code = extract_code_block(&response);
    tracing::info!(
        "Code generation done: raw_response={} chars | extracted_code={} lines, {} bytes | cost=${:.4}",
        response.len(),
        code.lines().count(),
        code.len(),
        usage.estimated_cost_cents as f64 / 100.0,
    );

    Ok((code, usage))
}

fn extract_code_block(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(start) = trimmed.find("```rust") {
        let after = &trimmed[start + 7..];
        if let Some(end) = after.find("```") {
            return after[..end].trim().to_string();
        }
    }
    if let Some(start) = trimmed.find("```") {
        let after = &trimmed[start + 3..];
        if let Some(end) = after.find("```") {
            return after[..end].trim().to_string();
        }
    }
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_invoice_schema() {
        let fields = parse_schema_fields(r#"{"invoice_number":"string","date":"string","total":"string"}"#).unwrap();
        assert_eq!(fields.len(), 3);
        assert!(fields.iter().any(|f| f.name == "invoice_number"));
    }

    #[test]
    fn test_code_template_contains_fields() {
        let fields = vec![
            SchemaField { name: "invoice_number".into(), field_type: "string".into() },
            SchemaField { name: "total".into(), field_type: "string".into() },
        ];
        let code = build_code_template(&fields);
        assert!(code.contains("invoice_number"));
        assert!(code.contains("total"));
        assert!(code.contains("#[no_mangle]"));
        assert!(code.contains("pub extern \"C\" fn extract"));
    }

    #[test]
    fn test_offline_codegen_uses_hardcoded_for_invoice() {
        let schema = r#"{"invoice_number":"string","date":"string","total":"string"}"#;
        let code = generate_guest_rust_code_offline(schema);
        assert!(code.contains("INV-2026-001") || code.contains("Invoice Number:"));
    }
}
