//! Precomputed per-document deterministic features.
//!
//! The text-quality gate, font stats, key-value detector, transaction-column
//! detector and columnar-table detector are each run by more than one stage of
//! the pipeline. Computing them once per document (instead of 2–4×) keeps a
//! single compile from repeating the same O(N log N) geometry work.

use crate::table_columns::TableColumn;
use crate::table_kv::KvField;
use optimus_core::{FontStats, TextQualityReport, TextSpan};

/// Deterministic analyses of a document's spans, computed once and shared.
#[derive(Debug, Clone)]
pub struct DocumentFeatures {
    pub font_stats: FontStats,
    pub quality: TextQualityReport,
    pub key_value_pairs: Vec<KvField>,
    pub transaction_columns: Vec<(String, String)>,
    pub table_columns: Option<Vec<TableColumn>>,
}

impl DocumentFeatures {
    pub fn compute(spans: &[TextSpan]) -> Self {
        Self {
            font_stats: optimus_core::calculate_font_stats(spans),
            quality: optimus_core::analyze_text_quality(spans),
            key_value_pairs: crate::table_kv::detect_key_value_pairs(spans),
            transaction_columns: crate::schema::detect_transactions_columns(spans),
            table_columns: crate::table_columns::detect_table_columns(spans),
        }
    }
}
