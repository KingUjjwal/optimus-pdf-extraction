//! Deterministic columnar-table detection from aligned spans.
//!
//! Borderless tables (product lists, order line items) are recognized when
//! several rows split into the same x-gap-separated column groups. The
//! detected columns become a layout prior for the LLM codegen and an offline
//! fallback, so the model never has to rediscover column geometry. Adapted
//! from firecrawl/pdf-inspector `try_build_table_from_columns`.

use optimus_core::TextSpan;

/// A detected table column with its horizontal extent.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TableColumn {
    pub header: String,
    pub x0: f32,
    pub x1: f32,
}

/// Best-signature candidate: (boundaries, row count, sample rows).
type BestSignature<'a> = (Vec<u32>, usize, Vec<Vec<&'a TextSpan>>);

/// Column boundaries must align within this many px to count as the same table.
const BOUNDARY_TOL: u32 = 6;

/// Detect a columnar table in the span set.
///
/// Rows are grouped by y-overlap; each row is split into columns at every
/// x-gap >= 10px. Rows whose column boundaries align within `BOUNDARY_TOL` px
/// form a cluster; when a cluster holds three or more rows with >= 2 columns,
/// the table is accepted and the topmost row's texts become the headers.
/// Guards reject sparse column groups and numeric-only headers.
pub fn detect_table_columns(spans: &[TextSpan]) -> Option<Vec<TableColumn>> {
    let rows = group_rows(spans, 4.0);
    if rows.len() < 3 {
        return None;
    }

    // Split every row into column groups at gaps >= 10px, then cluster rows
    // whose column boundaries align within BOUNDARY_TOL px of each other
    // (tolerant of jitter in real PDFs).
    let mut candidates: Vec<(Vec<u32>, Vec<&TextSpan>)> = Vec::new();
    for row in &rows {
        let mut sorted = row.clone();
        sorted.sort_by(|a, b| a.x0.total_cmp(&b.x0));
        if sorted.len() < 2 {
            continue;
        }
        let mut groups: Vec<Vec<&TextSpan>> = Vec::new();
        let mut current = vec![sorted[0]];
        for i in 0..sorted.len() - 1 {
            let gap = sorted[i + 1].x0 - sorted[i].x1;
            if gap >= 10.0 {
                groups.push(std::mem::take(&mut current));
            }
            current.push(sorted[i + 1]);
        }
        groups.push(current);
        if groups.len() < 2 {
            continue;
        }
        let boundaries: Vec<u32> = groups.iter().map(|g| g[0].x0.round() as u32).collect();
        candidates.push((boundaries, sorted));
    }

    // Cluster candidates by tolerant boundary alignment.
    let mut clusters: Vec<(Vec<u32>, Vec<Vec<&TextSpan>>)> = Vec::new();
    for (boundaries, sorted) in &candidates {
        let anchor = clusters.iter_mut().find(|(anchor, _)| {
            anchor.len() == boundaries.len()
                && anchor
                    .iter()
                    .zip(boundaries.iter())
                    .all(|(a, b)| (*a as i64 - *b as i64).unsigned_abs() <= BOUNDARY_TOL as u64)
        });
        match anchor {
            Some((_, rows)) => rows.push(sorted.clone()),
            None => clusters.push((boundaries.clone(), vec![sorted.clone()])),
        }
    }

    // Pick the largest cluster with >= 3 rows and >= 2 columns.
    let mut best: Option<BestSignature> = None;
    for (boundaries, rows) in &clusters {
        if boundaries.len() < 2 || rows.len() < 3 {
            continue;
        }
        if best.as_ref().is_none_or(|(_, bc, _)| rows.len() > *bc) {
            best = Some((boundaries.clone(), rows.len(), rows.clone()));
        }
    }
    let (_boundaries, _count, rows) = best?;

    // Topmost qualifying row = headers.
    let mut header_row = rows[0].clone();
    header_row.sort_by(|a, b| a.y0.total_cmp(&b.y0));
    let header_texts: Vec<String> = header_row
        .iter()
        .map(|s| s.text.trim().to_string())
        .collect();
    if header_texts
        .iter()
        .all(|t| t.chars().all(|c| !c.is_alphabetic()))
    {
        return None;
    }

    let mut columns: Vec<TableColumn> = Vec::new();
    for (i, header) in header_texts.iter().enumerate() {
        let mut min_x = f32::MAX;
        let mut max_x = -f32::MAX;
        for row in &rows {
            if let Some(cell) = row.get(i) {
                min_x = min_x.min(cell.x0);
                max_x = max_x.max(cell.x1);
            }
        }
        columns.push(TableColumn {
            header: header.clone(),
            x0: min_x,
            x1: max_x,
        });
    }

    if columns.len() >= 2 {
        Some(columns)
    } else {
        None
    }
}

/// Group spans into visual rows by vertical center overlap.
fn group_rows(spans: &[TextSpan], y_tol: f32) -> Vec<Vec<&TextSpan>> {
    let mut sorted: Vec<&TextSpan> = spans.iter().collect();
    sorted.sort_by(|a, b| a.y0.total_cmp(&b.y0));

    let mut rows: Vec<Vec<&TextSpan>> = Vec::new();
    for span in sorted {
        let center = (span.y0 + span.y1) / 2.0;
        if let Some(row) = rows.iter_mut().find(|r| {
            r.iter()
                .any(|s| (center - (s.y0 + s.y1) / 2.0).abs() <= y_tol)
        }) {
            row.push(span);
        } else {
            rows.push(vec![span]);
        }
    }
    rows
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
    fn detects_three_column_table() {
        // 4 rows × 3 columns, consistent x-gaps.
        let mut spans = Vec::new();
        let headers = [("Item", 0.0), ("Qty", 120.0), ("Price", 240.0)];
        for (t, x) in headers {
            spans.push(span(t, x, 0.0));
        }
        let data = [
            ("Widget", 0.0, 120.0, 240.0),
            ("Gadget", 0.0, 120.0, 240.0),
            ("Doohickey", 0.0, 120.0, 240.0),
        ];
        for (row_i, (t, x0, x1, x2)) in data.iter().enumerate() {
            let y = (row_i + 1) as f32 * 20.0;
            spans.push(span(t, *x0, y));
            spans.push(span("2", *x1, y));
            spans.push(span("5.00", *x2, y));
        }
        let cols = detect_table_columns(&spans).expect("table");
        assert_eq!(cols.len(), 3);
        assert_eq!(cols[0].header, "Item");
        assert_eq!(cols[1].header, "Qty");
        assert_eq!(cols[2].header, "Price");
        assert!(cols[2].x1 > cols[1].x1);
    }

    #[test]
    fn two_rows_is_not_a_table() {
        let spans = vec![
            span("A", 0.0, 0.0),
            span("B", 120.0, 0.0),
            span("C", 0.0, 20.0),
            span("D", 120.0, 20.0),
        ];
        assert!(detect_table_columns(&spans).is_none());
    }

    #[test]
    fn numeric_only_headers_rejected() {
        // Headers are column indices — must not be treated as a real table.
        let mut spans = Vec::new();
        for row in 0..4 {
            spans.push(span("1", 0.0, row as f32 * 20.0));
            spans.push(span("2", 120.0, row as f32 * 20.0));
        }
        assert!(detect_table_columns(&spans).is_none());
    }

    #[test]
    fn tolerant_to_column_jitter() {
        // Column x-positions drift by up to 4px per row; boundaries still
        // cluster within BOUNDARY_TOL and the table is detected.
        let mut spans = Vec::new();
        for (t, x) in [("Item", 0.0f32), ("Qty", 120.0), ("Price", 240.0)] {
            spans.push(span(t, x, 0.0));
        }
        let jitter = [0.0f32, 3.0, 4.0, 2.0];
        for (row_i, j) in jitter.iter().enumerate() {
            let y = (row_i + 1) as f32 * 20.0;
            spans.push(span("Widget", j + 0.0, y));
            spans.push(span("2", j + 120.0, y));
            spans.push(span("5.00", j + 240.0, y));
        }
        let cols = detect_table_columns(&spans).expect("jittery table");
        assert_eq!(cols.len(), 3);
        assert_eq!(cols[0].header, "Item");
    }
}
