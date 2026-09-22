pub mod codegen;
pub mod compiler;
pub mod config;
pub mod features;
pub mod geometry;
pub mod llm;
pub mod observability;
pub mod schema;
pub mod table_columns;
pub mod table_kv;
pub mod templates;

use optimus_core::SpatialGraph;
use std::path::Path;

pub use codegen::{
    generate_guest_rust_code, generate_guest_rust_code_offline, parse_schema_fields, SchemaField,
};
pub use compiler::{
    compile_extraction_logic_sync, compile_extraction_logic_sync_with_flat_graph, LayoutManifest,
    CACHE_VERSION,
};
pub use config::{
    CacheConfig, CompilationConfig, CoreConfig, CostTracker, LlmConfig, OptimusConfig,
    RuntimeConfig, TokenUsage,
};
pub use features::DocumentFeatures;
pub use llm::{create_provider, ChatProvider, LlmProvider};
pub use observability::{
    record_llm_call, LlmAggregateStats, LlmCallHistory, LlmCallRecord, LlmCallType, LlmErrorKind,
};
pub use schema::discover_schema_llm;
pub use schema::{infer_schema, infer_schema_with_features};
pub use table_columns::{detect_table_columns, TableColumn};
pub use table_kv::{detect_key_value_pairs, is_toc_entry, KvField};

#[tracing::instrument(skip_all)]
pub fn display_id(id: &str) -> &str {
    // Truncate at a char boundary: `&id[..16]` panics when byte 16 lands inside
    // a multi-byte UTF-8 char.
    match id.char_indices().nth(16) {
        Some((idx, _)) => &id[..idx],
        None => id,
    }
}

/// Serializes the Spatial Graph into a flat pipe-delimited layout text format.
/// Format: `node_text|top_neighbor|bottom_neighbor|left_neighbor|right_neighbor|x0|y0|x1|y1\n`
#[tracing::instrument(level = "debug", skip_all, fields(node_count = graph.nodes.len()))]
pub fn serialize_flat_graph(graph: &SpatialGraph) -> String {
    let sanitize = |t: &str| t.replace(['|', '\n'], "");
    // Neighbours are stored by index; resolve their text from the node list
    // (nodes are in span order, so a neighbour's index is a valid node index).
    let neighbor_text = |n: &Option<optimus_core::SpatialNeighbor>| -> String {
        n.as_ref()
            .and_then(|nb| graph.nodes.get(nb.index))
            .map(|node| sanitize(&node.span.text))
            .unwrap_or_else(|| "None".to_string())
    };

    let mut res = String::new();
    for node in &graph.nodes {
        let text = sanitize(&node.span.text);
        let top = neighbor_text(&node.nearest_top);
        let bot = neighbor_text(&node.nearest_bottom);
        let left = neighbor_text(&node.nearest_left);
        let right = neighbor_text(&node.nearest_right);

        res.push_str(&format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}\n",
            text, top, bot, left, right, node.span.x0, node.span.y0, node.span.x1, node.span.y1
        ));
    }
    res
}

/// Schema discovery from spans with heuristic inference.
/// Augments label:value inference with automatic transaction-table detection so
/// the resulting schema can drive array extraction (e.g. `transactions`).
#[tracing::instrument(level = "debug", skip_all, fields(span_count = spans.len()))]
pub fn discover_schema_from_spans(spans: &[optimus_core::TextSpan]) -> String {
    let features = DocumentFeatures::compute(spans);
    discover_schema_from_spans_with_features(spans, &features)
}

/// Like `discover_schema_from_spans`, but reuses precomputed document features.
#[tracing::instrument(level = "debug", skip_all, fields(span_count = spans.len()))]
pub fn discover_schema_from_spans_with_features(
    spans: &[optimus_core::TextSpan],
    features: &DocumentFeatures,
) -> String {
    let mut base: serde_json::Map<String, serde_json::Value> =
        schema::infer_schema_with_features(spans, features)
            .parse::<serde_json::Value>()
            .unwrap_or_else(|_| serde_json::json!({}))
            .as_object()
            .cloned()
            .unwrap_or_default();

    let cols = &features.transaction_columns;
    if !cols.is_empty() {
        let columns_obj: serde_json::Map<String, serde_json::Value> = cols
            .iter()
            .map(|(l, k)| (l.clone(), serde_json::Value::String(k.clone())))
            .collect();
        base.insert(
            "transactions".to_string(),
            serde_json::json!({ "type": "array", "columns": columns_obj }),
        );
    }

    serde_json::to_string(&serde_json::Value::Object(base))
        .unwrap_or_else(|_| schema::infer_schema(spans))
}

/// Backward-compatible synchronous compilation (wraps async version).
#[tracing::instrument(level = "info", skip(_graph, cache_dir), fields(layout_id = %layout_id))]
pub fn compile_extraction_logic(
    layout_id: &str,
    _graph: &SpatialGraph,
    schema: &str,
    cache_dir: &Path,
) -> anyhow::Result<Vec<u8>> {
    compile_extraction_logic_sync(layout_id, _graph, schema, cache_dir)
}

/// Returns true if a cached layout's manifest exists and matches the current CACHE_VERSION.
#[tracing::instrument(level = "debug", skip_all)]
pub fn manifest_cache_current(layout_id: &str, cache_dir: &Path) -> bool {
    let manifest_path = cache_dir.join(layout_id).join("manifest.json");
    match std::fs::read_to_string(&manifest_path) {
        Ok(s) => serde_json::from_str::<crate::compiler::LayoutManifest>(&s)
            .map(|m| m.cache_version == CACHE_VERSION)
            .unwrap_or(false),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use optimus_core::TextSpan;

    #[test]
    fn test_flat_serialization() {
        let spans = vec![
            TextSpan {
                text: "INVOICE".to_string(),
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
                text: "Invoice Number:".to_string(),
                x0: 50.0,
                y0: 700.0,
                x1: 150.0,
                y1: 715.0,
                page: None,
                font_size: 0.0,
                is_bold: false,
                is_italic: false,
            },
        ];
        let graph = optimus_core::build_spatial_graph(spans);
        let serialized = serialize_flat_graph(&graph);

        assert!(serialized.contains("INVOICE"));
        assert!(serialized.contains("Invoice Number:"));
    }

    #[test]
    fn display_id_truncates_at_char_boundary() {
        assert_eq!(display_id("short"), "short");
        assert_eq!(display_id(&"a".repeat(64)), "a".repeat(16));
        // Multi-byte chars: must not panic and must cut on a boundary.
        let multibyte = "é".repeat(40);
        let shown = display_id(&multibyte);
        assert_eq!(shown.chars().count(), 16);
        assert!(multibyte.starts_with(shown));
    }
}
