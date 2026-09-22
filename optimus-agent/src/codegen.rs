use crate::config::TokenUsage;
use crate::llm::LlmProvider;
use crate::observability::{record_llm_call, LlmCallHistory, LlmCallRecord, LlmCallType};
use crate::templates;
use anyhow::Result;

/// A parsed schema field with name and type.
pub struct SchemaField {
    pub name: String,
    pub field_type: String,
    /// For array fields: (document column label, output key) pairs.
    pub columns: Vec<(String, String)>,
}

/// Parses a schema into fields. Scalar fields are `"key": "type"` strings;
/// array fields are `"key": {"type": "array", "columns": {"Label": "key", ...}}`.
#[tracing::instrument(skip_all)]
pub fn parse_schema_fields(schema: &str) -> Result<Vec<SchemaField>> {
    let parsed: serde_json::Value = serde_json::from_str(schema)?;
    let obj = parsed
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("Schema is not a JSON object"))?;
    let mut fields = Vec::new();
    for (key, val) in obj {
        if let Some(cols) = val
            .as_object()
            .and_then(|o| o.get("columns"))
            .and_then(|c| c.as_object())
        {
            let mut columns: Vec<(String, String)> = cols
                .iter()
                .map(|(label, k)| (label.clone(), k.as_str().unwrap_or(label).to_string()))
                .collect();
            columns.sort_by(|a, b| a.0.cmp(&b.0));
            fields.push(SchemaField {
                name: key.clone(),
                field_type: "array".into(),
                columns,
            });
        } else {
            let field_type = val.as_str().unwrap_or("string").to_string();
            fields.push(SchemaField {
                name: key.clone(),
                field_type,
                columns: Vec::new(),
            });
        }
    }
    fields.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(fields)
}

fn build_label(name: &str) -> String {
    name.replace('_', " ")
        .split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn build_array_block(field: &SchemaField) -> String {
    let arr = &field.name;
    let cols_list: Vec<String> = field
        .columns
        .iter()
        .map(|(label, key)| format!("        (\"{}\", \"{}\"),", label, key))
        .collect();
    format!(
        r#"    // -- array field: {arr} -------------------------------------------------
    const {arr_upper}_COLS: [(&str, &str); {n}] = [
{cols_list}
    ];
    let mut {arr}_headers: Vec<&FlatGraphLine> = Vec::new();
    let mut {arr}_cols: Vec<(&str, &str)> = Vec::new();
    for (label, key) in {arr_upper}_COLS.iter() {{
        if let Some(h) = find_header(&graph, label) {{
            {arr}_headers.push(h);
            {arr}_cols.push((*label, *key));
        }}
    }}
    let mut {arr}_rows: Vec<Vec<(String, String)>> = Vec::new();
    for row in find_transaction_rows(&graph, 2.0) {{
        let obj = row_to_object(&row, &{arr}_cols, &{arr}_headers);
        {arr}_rows.push(obj);
    }}
"#,
        arr = arr,
        arr_upper = arr.to_uppercase(),
        n = field.columns.len(),
        cols_list = cols_list.join("\n"),
    )
}

fn build_code_template(fields: &[SchemaField]) -> String {
    if fields.is_empty() {
        return HARDCODED_INVOICE_CODE.to_string();
    }
    let scalar_fields: Vec<&SchemaField> =
        fields.iter().filter(|f| f.field_type != "array").collect();
    let array_fields: Vec<&SchemaField> =
        fields.iter().filter(|f| f.field_type == "array").collect();

    // Resolve all scalar labels in one indexed pass (`find_label_values`)
    // instead of one O(N) scan per field.
    let mut vars = String::new();
    for f in &scalar_fields {
        vars.push_str(&format!(
            "    let mut {} = String::from(\"Unknown\");\n",
            f.name
        ));
    }

    let mut if_chain = String::new();
    if !scalar_fields.is_empty() {
        let labels: Vec<String> = scalar_fields
            .iter()
            .map(|f| format!("\"{}:\"", build_label(&f.name)))
            .collect();
        if_chain.push_str(&format!("    let __labels = [{}];\n", labels.join(", ")));
        if_chain.push_str("    let mut __values = find_label_values(&graph, &__labels);\n");
        for (i, f) in scalar_fields.iter().enumerate() {
            if_chain.push_str(&format!(
                "    if let Some(val) = __values[{i}].take() {{\n        {} = val;\n    }}\n",
                f.name
            ));
        }
    }

    let mut array_blocks = String::new();
    for f in &array_fields {
        array_blocks.push_str(&build_array_block(f));
    }
    let mut emit_fields = String::new();
    for f in &scalar_fields {
        emit_fields.push_str(&format!(
            "        (\"{}\", JsonValue::Str({})),\n",
            f.name, f.name
        ));
    }
    for f in &array_fields {
        emit_fields.push_str(&format!(
            "        (\"{}\", JsonValue::Array({}_rows)),\n",
            f.name, f.name
        ));
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
{array_blocks}
    let json_bytes = emit_json_typed(&[
{emit_fields}    ]);

    let boxed = json_bytes.into_boxed_slice();
    Box::into_raw(boxed) as *mut u8
}}
"##,
        vars = vars,
        if_chain = if_chain,
        array_blocks = array_blocks,
        emit_fields = emit_fields,
    )
}

const HARDCODED_INVOICE_CODE: &str = include_str!("../resources/fallback_wasm.rs");

#[tracing::instrument(skip_all)]
pub fn generate_guest_rust_code_offline(schema: &str) -> String {
    let fields = parse_schema_fields(schema).unwrap_or_default();
    if fields.iter().all(|f| f.field_type != "array")
        && fields.iter().any(|f| f.name == "invoice_number")
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
    layout_priors: Option<&str>,
    provider: Option<&dyn LlmProvider>,
    history: Option<&LlmCallHistory>,
    event_tx: Option<&tokio::sync::mpsc::UnboundedSender<LlmCallRecord>>,
) -> Result<(String, TokenUsage)> {
    let Some(llm) = provider else {
        tracing::info!("Code generation: using OFFLINE template");
        let code = generate_guest_rust_code_offline(schema);
        tracing::info!(
            "Generated offline Rust code: {} lines, {} bytes",
            code.lines().count(),
            code.len()
        );
        return Ok((code, TokenUsage::default()));
    };

    tracing::info!(
        "Code generation: using LLM model={} | schema keys={} | graph chars={} | priors={}",
        llm.model_name(),
        schema.len(),
        flat_graph.len(),
        layout_priors.map_or(0, |p| p.len()),
    );

    let system = templates::CODE_GENERATION_SYSTEM.to_string();
    let user_prompt = templates::code_generation_user(schema, flat_graph, layout_priors);
    let (response, usage) = record_llm_call(
        llm,
        LlmCallType::CodeGeneration,
        &system,
        &user_prompt,
        history,
        event_tx,
    )
    .await?;

    tracing::debug!(
        "LLM raw codegen response ({} chars):\n{}",
        response.len(),
        response
    );

    let code = extract_code_block(&response);
    tracing::info!(
        "Code generation done: raw_response={} chars | extracted_code={} lines, {} bytes | cost=${:.4}",
        response.len(),
        code.lines().count(),
        code.len(),
        usage.estimated_cost_cents as f64 / 100.0,
    );

    if !code.contains("fn extract") {
        tracing::warn!(
            "codegen returned no `extract` function: raw_response={} chars, extracted_code={} lines — compile loop will attempt an LLM fix",
            response.len(),
            code.lines().count(),
        );
    }

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
        let fields =
            parse_schema_fields(r#"{"invoice_number":"string","date":"string","total":"string"}"#)
                .unwrap();
        assert_eq!(fields.len(), 3);
        assert!(fields.iter().any(|f| f.name == "invoice_number"));
    }

    #[test]
    fn test_code_template_contains_fields() {
        let fields = vec![
            SchemaField {
                name: "invoice_number".into(),
                field_type: "string".into(),
                columns: Vec::new(),
            },
            SchemaField {
                name: "total".into(),
                field_type: "string".into(),
                columns: Vec::new(),
            },
        ];
        let code = build_code_template(&fields);
        assert!(code.contains("invoice_number"));
        assert!(code.contains("total"));
        assert!(code.contains("#[no_mangle]"));
        assert!(code.contains("pub extern \"C\" fn extract"));
        assert!(code.contains("JsonValue::Str"));
    }

    #[test]
    fn test_parse_array_schema() {
        let schema = r#"{"client_name":"string","transactions":{"type":"array","columns":{"Date":"date","Amount":"amount","Transaction":"narration"}}}"#;
        let fields = parse_schema_fields(schema).unwrap();
        let txn = fields.iter().find(|f| f.name == "transactions").unwrap();
        assert_eq!(txn.field_type, "array");
        assert_eq!(txn.columns.len(), 3);
        assert!(txn.columns.iter().any(|(l, k)| l == "Date" && k == "date"));
        assert!(fields.iter().any(|f| f.name == "client_name"));
    }

    #[test]
    fn test_code_template_array_block() {
        let schema =
            r#"{"transactions":{"type":"array","columns":{"Date":"date","Amount":"amount"}}}"#;
        let fields = parse_schema_fields(schema).unwrap();
        let code = build_code_template(&fields);
        assert!(code.contains("TRANSACTIONS_COLS"));
        assert!(code.contains("find_transaction_rows"));
        assert!(code.contains("row_to_object"));
        assert!(code.contains("JsonValue::Array"));
    }

    #[test]
    fn test_offline_codegen_uses_hardcoded_for_invoice() {
        let schema = r#"{"invoice_number":"string","date":"string","total":"string"}"#;
        let code = generate_guest_rust_code_offline(schema);
        assert!(code.contains("INV-2026-001") || code.contains("Invoice Number:"));
    }

    #[test]
    fn test_extract_code_block_empty() {
        assert_eq!(extract_code_block(""), "");
        assert_eq!(extract_code_block("   \n  "), "");
    }

    #[test]
    fn test_extract_code_block_fence_only() {
        assert_eq!(extract_code_block("```rust\n```"), "");
        assert_eq!(extract_code_block("```\n```"), "");
    }

    #[test]
    fn test_extract_code_block_strips_fence() {
        let raw = "here is the code:\n```rust\nfn extract() {}\n```\nthanks";
        assert!(extract_code_block(raw).contains("fn extract"));
    }

    #[test]
    fn test_offline_codegen_always_contains_extract() {
        let code = generate_guest_rust_code_offline(r#"{"client_name":"string","total":"string"}"#);
        assert!(code.contains("fn extract"));
    }
}
