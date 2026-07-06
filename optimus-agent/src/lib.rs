pub mod config;
pub mod llm;
pub mod templates;
pub mod schema;
pub mod codegen;
pub mod compiler;
pub mod observability;

use optimus_core::SpatialGraph;
use std::path::Path;

pub use config::{OptimusConfig, LlmConfig, CompilationConfig, TokenUsage, CostTracker};
pub use llm::{LlmProvider, ChatProvider, create_provider};
pub use schema::discover_schema_llm;
pub use schema::infer_schema;
pub use codegen::{SchemaField, parse_schema_fields, generate_guest_rust_code, generate_guest_rust_code_offline};
pub use compiler::{LayoutManifest, compile_extraction_logic_sync};
pub use observability::{
    LlmCallRecord, LlmCallType, LlmErrorKind, LlmAggregateStats, LlmCallHistory, record_llm_call,
};

#[tracing::instrument(skip_all)]
pub fn display_id(id: &str) -> &str {
    &id[..16.min(id.len())]
}

/// Serializes the Spatial Graph into a flat pipe-delimited layout text format.
/// Format: `node_text|top_neighbor|bottom_neighbor|left_neighbor|right_neighbor\n`
#[tracing::instrument(level = "debug", skip_all, fields(node_count = graph.nodes.len()))]
pub fn serialize_flat_graph(graph: &SpatialGraph) -> String {
    let mut res = String::new();
    for node in &graph.nodes {
        let sanitize = |t: &str| t.replace('|', "").replace('\n', "");

        let text = sanitize(&node.span.text);
        let top = node.nearest_top.as_ref().map(|n| sanitize(&n.text)).unwrap_or_else(|| "None".to_string());
        let bot = node.nearest_bottom.as_ref().map(|n| sanitize(&n.text)).unwrap_or_else(|| "None".to_string());
        let left = node.nearest_left.as_ref().map(|n| sanitize(&n.text)).unwrap_or_else(|| "None".to_string());
        let right = node.nearest_right.as_ref().map(|n| sanitize(&n.text)).unwrap_or_else(|| "None".to_string());

        res.push_str(&format!("{}|{}|{}|{}|{}\n", text, top, bot, left, right));
    }
    res
}

/// Schema discovery from spans with heuristic inference.
#[tracing::instrument(level = "debug", skip_all, fields(span_count = spans.len()))]
pub fn discover_schema_from_spans(spans: &[optimus_core::TextSpan]) -> String {
    schema::infer_schema(spans)
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

#[cfg(test)]
mod tests {
    use super::*;
    use optimus_core::TextSpan;

    #[test]
    fn test_flat_serialization() {
        let spans = vec![
            TextSpan { text: "INVOICE".to_string(), x0: 50.0, y0: 750.0, x1: 150.0, y1: 770.0 },
            TextSpan { text: "Invoice Number:".to_string(), x0: 50.0, y0: 700.0, x1: 150.0, y1: 715.0 },
        ];
        let graph = optimus_core::build_spatial_graph(spans);
        let serialized = serialize_flat_graph(&graph);

        assert!(serialized.contains("INVOICE"));
        assert!(serialized.contains("Invoice Number:"));
    }
}
