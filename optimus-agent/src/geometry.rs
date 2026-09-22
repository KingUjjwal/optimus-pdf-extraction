//! Shared span geometry helpers used by the deterministic table detectors.
//!
//! `group_rows` was previously duplicated (and quadratic) in both `table_kv`
//! and `table_columns`: for every span it scanned every existing row. Sorting by
//! y and accumulating in a single pass makes it O(N log N) and keeps one
//! implementation.

use optimus_core::TextSpan;

/// Groups spans into visual rows by vertical-center proximity. Spans are sorted
/// by `y0`, then accumulated in one pass: a span joins the current row when its
/// center is within `y_tol` of the row's reference center, otherwise it starts a
/// new row. Rows are returned in ascending-y order.
pub fn group_rows(spans: &[TextSpan], y_tol: f32) -> Vec<Vec<&TextSpan>> {
    let mut sorted: Vec<&TextSpan> = spans.iter().collect();
    sorted.sort_by(|a, b| a.y0.total_cmp(&b.y0));

    let mut rows: Vec<Vec<&TextSpan>> = Vec::new();
    let mut row_ref_y = f32::NAN;
    for span in sorted {
        let center = (span.y0 + span.y1) / 2.0;
        match rows.last_mut() {
            Some(row) if (center - row_ref_y).abs() <= y_tol => row.push(span),
            _ => {
                row_ref_y = center;
                rows.push(vec![span]);
            }
        }
    }
    rows
}

/// True when a cell looks like a `DD-MMM-YYYY` transaction date
/// (e.g. `26-Jun-2025`). Shared by transaction-column and table detection.
pub fn looks_like_date(s: &str) -> bool {
    let s = s.trim();
    let parts: Vec<&str> = s.split('-').collect();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn span(text: &str, y: f32) -> TextSpan {
        TextSpan {
            text: text.into(),
            x0: 0.0,
            y0: y,
            x1: 10.0,
            y1: y + 10.0,
            page: Some(1),
            font_size: 10.0,
            is_bold: false,
            is_italic: false,
        }
    }

    #[test]
    fn groups_spans_sharing_a_band() {
        let spans = vec![
            span("a", 100.0),
            span("b", 101.0),
            span("c", 120.0),
            span("d", 120.5),
        ];
        let rows = group_rows(&spans, 4.0);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].len(), 2);
        assert_eq!(rows[1].len(), 2);
    }

    #[test]
    fn dates() {
        assert!(looks_like_date("26-Jun-2025"));
        assert!(looks_like_date("1-Jan-2025"));
        assert!(!looks_like_date("2026-05-23"));
        assert!(!looks_like_date("Total"));
        assert!(!looks_like_date(""));
    }
}
