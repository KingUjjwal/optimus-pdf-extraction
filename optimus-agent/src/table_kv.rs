//! Deterministic key-value (`Field: Value`) detection from text spans.
//!
//! Invoice/statement headers are usually rendered as a short label span
//! followed by a value span separated by a wide horizontal gap, or as a
//! single `Label: value` inline span. This module detects both layouts
//! without an LLM, giving schema inference a strong prior and the offline
//! codegen path a deterministic fallback. Adapted from firecrawl/pdf-inspector
//! `try_build_key_value_table_from_rows`.

use crate::geometry::group_rows;
use optimus_core::TextSpan;

/// A detected `Label: value` pair.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct KvField {
    pub label: String,
    pub value: String,
}

/// Detect key-value pairs in a span set.
///
/// Two layouts are recognized per visual row:
/// - a single span containing a colon: `"Invoice Number: INV-001"`
/// - two (or more) spans separated by a wide x-gap: `Invoice Number` + `INV-001`
///
/// Guards keep prose and transaction-table rows out: labels are short,
/// alpha-containing, and deduplicated; values are non-empty.
pub fn detect_key_value_pairs(spans: &[TextSpan]) -> Vec<KvField> {
    let rows = group_rows(spans, 4.0);
    let mut fields: Vec<KvField> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for row in &rows {
        // Single span with inline colon.
        if row.len() == 1 {
            if let Some(pair) = split_inline_colon(&row[0].text) {
                push_unique(&mut fields, &mut seen, pair);
            }
            continue;
        }

        // Multi-span row: find the widest gap between consecutive spans.
        let mut sorted = row.clone();
        sorted.sort_by(|a, b| a.x0.total_cmp(&b.x0));

        let mut best_gap: Option<(usize, f32)> = None;
        for i in 0..sorted.len() - 1 {
            let gap = sorted[i + 1].x0 - sorted[i].x1;
            if best_gap.is_none_or(|(_, g)| gap > g) {
                best_gap = Some((i, gap));
            }
        }
        let Some((split_idx, gap)) = best_gap else {
            continue;
        };
        if gap < 10.0 {
            continue;
        }

        let label = join_spans(&sorted[..=split_idx]);
        let label = label.trim_end_matches(':').trim().to_string();
        let value = join_spans(&sorted[split_idx + 1..]);
        // TOC guard: a wide-gap row whose right side is a bare page number
        // ("Introduction ..... 5") is a table-of-contents entry, not a field.
        if is_toc_entry(&label, &value) {
            continue;
        }
        push_unique(&mut fields, &mut seen, KvField { label, value });
    }

    fields
}

/// True when a wide-gap pair is a table-of-contents entry rather than a real
/// field: the value is a bare page number (1-4 digits) and the label carries
/// no colon signal (TOC entries read "Introduction ..... 5").
pub fn is_toc_entry(label: &str, value: &str) -> bool {
    let value_trimmed = value.trim();
    let is_page_number = !value_trimmed.is_empty()
        && value_trimmed.chars().all(|c| c.is_ascii_digit())
        && value_trimmed.len() <= 4;
    is_page_number && !label.trim_end().ends_with(':')
}

/// Split a single `Label: value` span at the first colon.
fn split_inline_colon(text: &str) -> Option<KvField> {
    let idx = text.find(':')?;
    let label = text[..idx].trim();
    let value = text[idx + 1..].trim();
    if label.is_empty()
        || label.len() > 40
        || !label.chars().any(|c| c.is_alphabetic())
        || value.is_empty()
    {
        return None;
    }
    Some(KvField {
        label: label.to_string(),
        value: value.to_string(),
    })
}

/// Join span texts with single spaces, trimming whitespace.
fn join_spans(group: &[&TextSpan]) -> String {
    group
        .iter()
        .map(|s| s.text.trim())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn push_unique(
    fields: &mut Vec<KvField>,
    seen: &mut std::collections::HashSet<String>,
    pair: KvField,
) {
    let key = pair.label.to_lowercase();
    if seen.insert(key) {
        fields.push(pair);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(text: &str, x0: f32, y: f32) -> TextSpan {
        let w = text.chars().count() as f32 * 5.0;
        TextSpan {
            text: text.into(),
            x0,
            y0: y,
            x1: x0 + w,
            y1: y + 12.0,
            page: Some(1),
            font_size: 10.0,
            is_bold: false,
            is_italic: false,
        }
    }

    #[test]
    fn inline_colon_split() {
        let spans = vec![span("Invoice Number: INV-2026-001", 0.0, 0.0)];
        let pairs = detect_key_value_pairs(&spans);
        assert_eq!(
            pairs,
            vec![KvField {
                label: "Invoice Number".into(),
                value: "INV-2026-001".into()
            }]
        );
    }

    #[test]
    fn two_span_wide_gap_split() {
        let spans = vec![
            span("Invoice Number", 0.0, 0.0),
            span("INV-2026-001", 120.0, 0.0),
        ];
        let pairs = detect_key_value_pairs(&spans);
        assert_eq!(
            pairs,
            vec![KvField {
                label: "Invoice Number".into(),
                value: "INV-2026-001".into()
            }]
        );
    }

    #[test]
    fn narrow_gap_is_not_a_split() {
        // Two spans close together (e.g. a wrapped phrase) must not split.
        let spans = vec![span("Same", 0.0, 0.0), span("Phrase", 25.0, 0.0)];
        let pairs = detect_key_value_pairs(&spans);
        assert!(pairs.is_empty());
    }

    #[test]
    fn distinct_rows_are_separate_pairs() {
        let spans = vec![
            span("Invoice Number:", 0.0, 0.0),
            span("INV-001", 200.0, 0.0),
            span("Date:", 0.0, 20.0),
            span("2026-05-23", 200.0, 20.0),
        ];
        let pairs = detect_key_value_pairs(&spans);
        assert_eq!(pairs.len(), 2);
        assert!(pairs
            .iter()
            .any(|p| p.label == "Date" && p.value == "2026-05-23"));
    }

    #[test]
    fn digit_only_label_is_rejected() {
        let spans = vec![span("2026: 55", 0.0, 0.0)];
        let pairs = detect_key_value_pairs(&spans);
        assert!(pairs.is_empty());
    }

    #[test]
    fn duplicate_labels_deduplicated() {
        let spans = vec![
            span("Total:", 0.0, 0.0),
            span("$100", 200.0, 0.0),
            span("total:", 0.0, 20.0),
            span("$101", 200.0, 20.0),
        ];
        let pairs = detect_key_value_pairs(&spans);
        assert_eq!(pairs.len(), 1);
    }

    #[test]
    fn toc_page_number_rows_rejected() {
        // TOC entry: label + bare page number across a wide gap.
        let spans = vec![span("Introduction", 0.0, 0.0), span("5", 200.0, 0.0)];
        let pairs = detect_key_value_pairs(&spans);
        assert!(pairs.is_empty(), "TOC entry became a field: {pairs:?}");
    }

    #[test]
    fn is_toc_entry_detection() {
        assert!(is_toc_entry("Introduction", "5"));
        assert!(is_toc_entry("Chapter 2", "12"));
        assert!(!is_toc_entry("Total:", "500")); // colon label keeps it
        assert!(!is_toc_entry("Date", "2026-05-23"));
    }
}
