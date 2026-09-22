/// A single line from the flat spatial graph.
/// Format: text|top_neighbor|bottom_neighbor|left_neighbor|right_neighbor|x0|y0|x1|y1
/// The four trailing coordinates are optional — legacy 5-field lines default to 0.0.
#[derive(Debug, Clone)]
pub struct FlatGraphLine {
    pub text: String,
    pub top: String,
    pub bottom: String,
    pub left: String,
    pub right: String,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

fn parse_coord(s: Option<&str>, default: f32) -> f32 {
    s.and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(default)
}

/// Parses the flat spatial graph format into a Vec of lines.
pub fn parse_flat_graph(input: &str) -> Vec<FlatGraphLine> {
    let mut lines = Vec::new();
    for line_str in input.lines() {
        let parts: Vec<&str> = line_str.splitn(9, '|').collect();
        if parts.len() >= 5 {
            lines.push(FlatGraphLine {
                text: parts[0].to_string(),
                top: parts[1].to_string(),
                bottom: parts[2].to_string(),
                left: parts[3].to_string(),
                right: parts[4].to_string(),
                x0: parse_coord(parts.get(5).copied(), 0.0),
                y0: parse_coord(parts.get(6).copied(), 0.0),
                x1: parse_coord(parts.get(7).copied(), 0.0),
                y1: parse_coord(parts.get(8).copied(), 0.0),
            });
        }
    }
    lines
}

/// Finds the right neighbor of a node matching the given anchor text.
pub fn find_right_of(graph: &[FlatGraphLine], anchor: &str) -> Option<String> {
    graph.iter().find(|l| l.text == anchor).and_then(|l| {
        if l.right != "None" {
            Some(l.right.clone())
        } else {
            None
        }
    })
}

/// Finds the left neighbor of a node matching the given anchor text.
pub fn find_left_of(graph: &[FlatGraphLine], anchor: &str) -> Option<String> {
    graph.iter().find(|l| l.text == anchor).and_then(|l| {
        if l.left != "None" {
            Some(l.left.clone())
        } else {
            None
        }
    })
}

/// Finds the bottom neighbor of a node matching the given anchor text.
pub fn find_below(graph: &[FlatGraphLine], anchor: &str) -> Option<String> {
    graph.iter().find(|l| l.text == anchor).and_then(|l| {
        if l.bottom != "None" {
            Some(l.bottom.clone())
        } else {
            None
        }
    })
}

/// Finds the top neighbor of a node matching the given anchor text.
pub fn find_top_of(graph: &[FlatGraphLine], anchor: &str) -> Option<String> {
    graph.iter().find(|l| l.text == anchor).and_then(|l| {
        if l.top != "None" {
            Some(l.top.clone())
        } else {
            None
        }
    })
}

fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

/// Builds a JSON string from key-value pairs.
pub fn build_json(fields: &[(&str, &str)]) -> String {
    let mut s = String::from("{");
    for (i, (key, val)) in fields.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        s.push('"');
        s.push_str(&escape_json(key));
        s.push_str("\":\"");
        s.push_str(&escape_json(val));
        s.push('"');
    }
    s.push('}');
    s
}

/// Serializes extraction result with 4-byte little-endian length prefix.
pub fn emit_json(fields: &[(&str, &str)]) -> Vec<u8> {
    let json_str = build_json(fields);
    let bytes = json_str.as_bytes();
    let len = bytes.len() as u32;
    let mut out = Vec::with_capacity(4 + bytes.len());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(bytes);
    out
}

/// A typed JSON field value: a scalar string, or an array of homogeneous row objects.
pub enum JsonValue {
    Str(String),
    /// Vec<rows>, each row is Vec<(output_key, value)>.
    Array(Vec<Vec<(String, String)>>),
}

/// Builds a JSON object string with mixed scalar/array fields, e.g.
/// `{"client_name":"Alice","transactions":[{"date":"01-Jan-2026",...},...]}`.
pub fn build_json_typed(fields: &[(&str, JsonValue)]) -> String {
    let mut s = String::from("{");
    let mut first = true;
    for (key, val) in fields {
        if !first {
            s.push_str(", ");
        }
        first = false;
        s.push('"');
        s.push_str(&escape_json(key));
        s.push_str("\":");
        match val {
            JsonValue::Str(v) => {
                s.push('"');
                s.push_str(&escape_json(v));
                s.push('"');
            }
            JsonValue::Array(rows) => {
                s.push('[');
                for (i, row) in rows.iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    s.push('{');
                    for (j, (rk, rv)) in row.iter().enumerate() {
                        if j > 0 {
                            s.push_str(", ");
                        }
                        s.push('"');
                        s.push_str(&escape_json(rk));
                        s.push_str("\":\"");
                        s.push_str(&escape_json(rv));
                        s.push('"');
                    }
                    s.push('}');
                }
                s.push(']');
            }
        }
    }
    s.push('}');
    s
}

/// Serializes a typed extraction result with 4-byte little-endian length prefix.
pub fn emit_json_typed(fields: &[(&str, JsonValue)]) -> Vec<u8> {
    let json_str = build_json_typed(fields);
    let bytes = json_str.as_bytes();
    let len = bytes.len() as u32;
    let mut out = Vec::with_capacity(4 + bytes.len());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(bytes);
    out
}

/// Like `emit_json_typed`, but carries a per-field confidence level
/// (`"exact"` | `"heuristic"` | `"fuzzy"`) in a `_confidence` sibling object.
/// Additive and non-breaking: hosts that ignore `_confidence` still get the
/// same flat field values; hosts that read it can surface extraction quality.
pub fn emit_json_typed_with_confidence(
    fields: &[(&str, JsonValue)],
    confidence: &[(&str, &str)],
) -> Vec<u8> {
    let mut json = String::from("{\"_confidence\":{");
    let mut first = true;
    for (key, level) in confidence {
        if !first {
            json.push_str(", ");
        }
        first = false;
        json.push('"');
        json.push_str(&escape_json(key));
        json.push_str("\":\"");
        json.push_str(&escape_json(level));
        json.push('"');
    }
    json.push_str("}, ");
    let body = build_json_typed(fields);
    json.push_str(body.trim_start_matches('{'));
    let bytes = json.as_bytes();
    let len = bytes.len() as u32;
    let mut out = Vec::with_capacity(4 + bytes.len());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(bytes);
    out
}

/// First exact-text match for a column header. Unlike `find_*`, resolution is
/// unambiguous by design (headers appear exactly once per table).
pub fn find_header<'a>(graph: &'a [FlatGraphLine], label: &str) -> Option<&'a FlatGraphLine> {
    graph.iter().find(|l| l.text == label)
}

/// Groups lines strictly below `above_y` into rows by y-center band tolerance.
/// Rows are returned top-to-bottom (ascending y). A row is a set of spans sharing
/// roughly the same vertical position — i.e. one physical table line.
pub fn rows_below(graph: &[FlatGraphLine], above_y: f32, y_tol: f32) -> Vec<Vec<&FlatGraphLine>> {
    let mut below: Vec<&FlatGraphLine> = graph.iter().filter(|l| l.y0 > above_y).collect();
    below.sort_by(|a, b| a.y0.partial_cmp(&b.y0).unwrap_or(std::cmp::Ordering::Equal));

    let mut rows: Vec<Vec<&FlatGraphLine>> = Vec::new();
    let mut cur: Vec<&FlatGraphLine> = Vec::new();
    let mut row_y = f32::MAX;
    for l in below {
        if cur.is_empty() {
            row_y = l.y0;
            cur.push(l);
        } else if (l.y0 - row_y).abs() <= y_tol {
            cur.push(l);
        } else {
            rows.push(std::mem::take(&mut cur));
            row_y = l.y0;
            cur.push(l);
        }
    }
    if !cur.is_empty() {
        rows.push(cur);
    }
    rows
}

/// Finds transaction rows page-independently by anchoring on date-like cells.
/// Every line whose text looks like a `DD-MMM-YYYY` date starts a row; the row is
/// the set of lines sharing its vertical band. Rows are returned in reading order:
/// page order (pages accumulate y offsets, so ascending y0), top-first within each
/// page (y grows upward on a page, so within-page rows are reversed).
pub fn find_transaction_rows(graph: &[FlatGraphLine], y_tol: f32) -> Vec<Vec<&FlatGraphLine>> {
    let mut bands: Vec<f32> = Vec::new();
    let mut rows: Vec<Vec<&FlatGraphLine>> = Vec::new();
    for l in graph {
        if !looks_like_date(l.text.trim()) {
            continue;
        }
        if bands.iter().any(|b| (l.y0 - b).abs() <= y_tol) {
            continue;
        }
        bands.push(l.y0);
        let row: Vec<&FlatGraphLine> = graph
            .iter()
            .filter(|o| (o.y0 - l.y0).abs() <= y_tol)
            .collect();
        rows.push(row);
    }
    rows.sort_by(|a, b| {
        a[0].y0
            .partial_cmp(&b[0].y0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut out: Vec<Vec<&FlatGraphLine>> = Vec::new();
    let mut page: Vec<Vec<&FlatGraphLine>> = Vec::new();
    let mut prev_y = f32::MIN;
    for row in rows {
        if !page.is_empty() && (row[0].y0 - prev_y).abs() > 100.0 {
            page.reverse();
            out.extend(page.iter().cloned());
            page.clear();
        }
        prev_y = row[0].y0;
        page.push(row);
    }
    page.reverse();
    out.extend(page);
    out
}

/// Picks the cell in `row` whose x-center is closest to `header`'s x-center.
/// Returns None when the row has no span in that column's band.
pub fn cell_for_header<'a>(row: &[&'a FlatGraphLine], header: &FlatGraphLine) -> Option<&'a str> {
    let header_cx = (header.x0 + header.x1) / 2.0;
    let mut best: Option<&FlatGraphLine> = None;
    let mut best_dist = f32::MAX;
    for l in row {
        let cx = (l.x0 + l.x1) / 2.0;
        let dist = (cx - header_cx).abs();
        if dist < best_dist {
            best_dist = dist;
            best = Some(l);
        }
    }
    best.map(|l| l.text.as_str())
}

/// Renders a row object for the given column (label, key) pairs using `cell_for_header`.
pub fn row_to_object(
    row: &[&FlatGraphLine],
    columns: &[(&str, &str)],
    headers: &[&FlatGraphLine],
) -> Vec<(String, String)> {
    let mut obj = Vec::new();
    for (i, (_, key)) in columns.iter().enumerate() {
        let h = headers.get(i).copied();
        let val = match h {
            Some(h) => cell_for_header(row, h).unwrap_or("Unknown").to_string(),
            None => "Unknown".to_string(),
        };
        obj.push((key.to_string(), val));
    }
    obj
}

/// True when a cell looks like a `DD-MMM-YYYY` transaction date.
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

/// ASCII-case-insensitive `starts_with` without allocating a lowercased copy.
fn starts_with_ignore_ascii_case(haystack: &str, prefix: &str) -> bool {
    haystack.len() >= prefix.len()
        && haystack.is_char_boundary(prefix.len())
        && haystack[..prefix.len()].eq_ignore_ascii_case(prefix)
}

/// Extracts a label's value from the graph. Handles both layouts:
/// a standalone `Label:` line with a right-neighbor value, or an inline
/// `Label: value` span (including leading whitespace and case variations).
/// Returns None when no non-empty value is found.
///
/// Uses ASCII-case-insensitive comparison instead of `to_lowercase()` per line:
/// the old form allocated two Strings for every graph line on every call, which
/// dominated extraction cost on large documents.
pub fn find_label_value(graph: &[FlatGraphLine], label: &str) -> Option<String> {
    let label = label.trim();
    for l in graph {
        if l.text.trim().eq_ignore_ascii_case(label) && l.right != "None" && !l.right.is_empty() {
            return Some(l.right.clone());
        }
    }
    for l in graph {
        let t = l.text.trim();
        if starts_with_ignore_ascii_case(t, label) {
            if let Some(idx) = t.find(':') {
                let v = t[idx + 1..].trim();
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

/// Resolves many labels in one pass. Builds a lowercased text index once
/// (`BTreeMap`, so no OS randomness is required in the wasm guest) making the
/// common exact-match case O(log N) per label instead of a full O(N) scan for
/// every field. Labels not found in the index fall back to `find_label_value`,
/// which also handles inline `Label: value` spans.
pub fn find_label_values(graph: &[FlatGraphLine], labels: &[&str]) -> Vec<Option<String>> {
    let mut index: std::collections::BTreeMap<String, &FlatGraphLine> =
        std::collections::BTreeMap::new();
    for l in graph {
        let key = l.text.trim().to_lowercase();
        if !key.is_empty() {
            index.entry(key).or_insert(l);
        }
    }
    labels
        .iter()
        .map(|label| {
            let key = label.trim().to_lowercase();
            if let Some(l) = index.get(&key) {
                if l.right != "None" && !l.right.is_empty() {
                    return Some(l.right.clone());
                }
            }
            find_label_value(graph, label)
        })
        .collect()
}

/// Guest memory allocator — uses Box<[u8]> for sound deallocation via free_buf.
#[no_mangle]
pub extern "C" fn alloc(size: usize) -> *mut u8 {
    let mut buf: Box<[u8]> = vec![0u8; size].into_boxed_slice();
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// Guest memory deallocator — must only be called on pointers from alloc() or Box::from_raw.
///
/// # Safety
/// `ptr` must be a pointer previously returned by `alloc` (or `Box::into_raw`) and
/// `len` must exactly match the allocation length. Passing arbitrary pointers or
/// lengths is undefined behavior.
#[no_mangle]
pub unsafe extern "C" fn free_buf(ptr: *mut u8, len: usize) {
    if !ptr.is_null() && len > 0 {
        let slice = std::ptr::slice_from_raw_parts_mut(ptr, len);
        let _ = Box::from_raw(slice);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_flat_graph() {
        let input = "INVOICE|None|Invoice Number:|None|None\nInvoice Number:|INVOICE|Date:|None|INV-001\nDate:|Invoice Number:|None|None|2026-01-01\nINV-001|None|None|Invoice Number:|None\n2026-01-01|None|None|Date:|None\n";
        let graph = parse_flat_graph(input);
        assert_eq!(graph.len(), 5);
        assert_eq!(graph[0].text, "INVOICE");
    }

    #[test]
    fn test_find_right_of() {
        let input = "Invoice Number:|None|None|None|INV-2026-001\n";
        let graph = parse_flat_graph(input);
        let val = find_right_of(&graph, "Invoice Number:");
        assert_eq!(val, Some("INV-2026-001".into()));
    }

    #[test]
    fn test_find_right_of_none() {
        let input = "Total:|None|None|None|None\n";
        let graph = parse_flat_graph(input);
        let val = find_right_of(&graph, "Total:");
        assert_eq!(val, None);
    }

    #[test]
    fn test_find_below() {
        let input =
            "INVOICE|None|Invoice Number:|None|None\nInvoice Number:|INVOICE|None|None|None\n";
        let graph = parse_flat_graph(input);
        let val = find_below(&graph, "INVOICE");
        assert_eq!(val, Some("Invoice Number:".into()));
    }

    #[test]
    fn test_find_left_of() {
        let input = "INV-2026-001|None|None|Invoice Number:|None\n";
        let graph = parse_flat_graph(input);
        let val = find_left_of(&graph, "INV-2026-001");
        assert_eq!(val, Some("Invoice Number:".into()));
    }

    #[test]
    fn test_find_top_of() {
        let input = "Invoice Number:|INVOICE|None|None|None\n";
        let graph = parse_flat_graph(input);
        let val = find_top_of(&graph, "Invoice Number:");
        assert_eq!(val, Some("INVOICE".into()));
    }

    #[test]
    fn test_find_top_of_none() {
        let input = "Total:|None|None|None|None\n";
        let graph = parse_flat_graph(input);
        let val = find_top_of(&graph, "Total:");
        assert_eq!(val, None);
    }

    #[test]
    fn test_find_left_of_none() {
        let input = "Solo:|None|None|None|None\n";
        let graph = parse_flat_graph(input);
        let val = find_left_of(&graph, "Solo:");
        assert_eq!(val, None);
    }

    #[test]
    fn test_emit_json() {
        let result = emit_json(&[("invoice_number", "INV-001"), ("total", "$500")]);
        assert!(result.len() > 4);
        let len = u32::from_le_bytes([result[0], result[1], result[2], result[3]]) as usize;
        let json = String::from_utf8(result[4..4 + len].to_vec()).unwrap();
        assert!(json.contains("invoice_number"));
        assert!(json.contains("INV-001"));
    }

    #[test]
    fn test_parse_flat_graph_9_field() {
        let input = "Date|None|None|None|26-Jun-2025|28.9|100.0|66.4|109.4\n";
        let graph = parse_flat_graph(input);
        assert_eq!(graph.len(), 1);
        assert_eq!(graph[0].text, "Date");
        assert_eq!(graph[0].right, "26-Jun-2025");
        assert!((graph[0].x0 - 28.9).abs() < 0.001);
        assert!((graph[0].y0 - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_find_header() {
        let input =
            "Date|a|b|c|d|10.0|100.0|20.0|105.0\nTransaction|x|y|z|w|30.0|100.0|40.0|105.0\n";
        let graph = parse_flat_graph(input);
        assert!(find_header(&graph, "Date").is_some());
        assert!(find_header(&graph, "Missing").is_none());
    }

    #[test]
    fn test_rows_below_groups_by_band() {
        let input = "H1|None|None|None|None|10.0|100.0|20.0|105.0\n\
                     H2|None|None|None|None|30.0|100.0|40.0|105.0\n\
                     r1|None|None|None|None|10.0|120.0|20.0|125.0\n\
                     r2|None|None|None|None|30.0|121.0|40.0|126.0\n\
                     r3|None|None|None|None|10.0|140.0|20.0|145.0\n";
        let graph = parse_flat_graph(input);
        let rows = rows_below(&graph, 110.0, 3.0);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].len(), 2);
        assert_eq!(rows[1].len(), 1);
        assert_eq!(rows[0][0].text, "r1");
        assert_eq!(rows[1][0].text, "r3");
    }

    #[test]
    fn test_cell_for_header_picks_nearest_column() {
        let input = "date_h|None|None|None|None|10.0|100.0|20.0|105.0\n\
                     amount_h|None|None|None|None|40.0|100.0|50.0|105.0\n\
                     row_date|None|None|None|None|12.0|120.0|22.0|125.0\n\
                     row_amount|None|None|None|None|42.0|120.0|50.0|125.0\n";
        let graph = parse_flat_graph(input);
        let date_h = find_header(&graph, "date_h").unwrap();
        let amount_h = find_header(&graph, "amount_h").unwrap();
        let rows = rows_below(&graph, 110.0, 3.0);
        let row = &rows[0];
        assert_eq!(cell_for_header(row, date_h), Some("row_date"));
        assert_eq!(cell_for_header(row, amount_h), Some("row_amount"));
    }

    #[test]
    fn test_emit_json_typed_with_array() {
        let fields = [
            ("client_name", JsonValue::Str("Ujjwal".to_string())),
            (
                "transactions",
                JsonValue::Array(vec![vec![
                    ("date".to_string(), "26-Jun-2025".to_string()),
                    ("amount".to_string(), "7,999.60".to_string()),
                ]]),
            ),
        ];
        let result = emit_json_typed(&fields);
        let len = u32::from_le_bytes([result[0], result[1], result[2], result[3]]) as usize;
        let json = String::from_utf8(result[4..4 + len].to_vec()).unwrap();
        assert!(json.contains("\"client_name\":\"Ujjwal\""));
        assert!(json.contains("\"transactions\":[{\"date\":\"26-Jun-2025\""));
        assert!(json.contains("\"amount\":\"7,999.60\"}]"));
    }

    #[test]
    fn test_emit_json_typed_with_confidence() {
        let fields = [("invoice_number", JsonValue::Str("INV-001".to_string()))];
        let confidence = [("invoice_number", "exact")];
        let result = emit_json_typed_with_confidence(&fields, &confidence);
        let len = u32::from_le_bytes([result[0], result[1], result[2], result[3]]) as usize;
        let json = String::from_utf8(result[4..4 + len].to_vec()).unwrap();
        assert!(json.contains("\"_confidence\":{\"invoice_number\":\"exact\"}"));
        assert!(json.contains("\"invoice_number\":\"INV-001\""));
    }

    #[test]
    fn test_looks_like_date() {
        assert!(looks_like_date("26-Jun-2025"));
        assert!(looks_like_date("1-Jan-2025"));
        assert!(!looks_like_date("Total"));
        assert!(!looks_like_date("Page 2 of 4"));
        assert!(!looks_like_date("2026-05-23"));
        assert!(!looks_like_date(""));
    }

    #[test]
    fn test_find_label_value() {
        let input = "Email Id: ujjwal@example.com|None|None|None|None|10.0|100.0|20.0|105.0\n\
                     Mobile:|None|None|None|+919045473158|10.0|90.0|20.0|95.0\n\
                      Nominee 1: DEVENDRA KUMAR|None|None|None|None|10.0|80.0|20.0|85.0\n\
                     PAN: AMXPU9247Q|None|None|None|None|10.0|70.0|20.0|75.0\n";
        let graph = parse_flat_graph(input);
        assert_eq!(
            find_label_value(&graph, "Email Id:").as_deref(),
            Some("ujjwal@example.com")
        );
        assert_eq!(
            find_label_value(&graph, "Mobile:").as_deref(),
            Some("+919045473158")
        );
        assert_eq!(
            find_label_value(&graph, "Nominee 1:").as_deref(),
            Some("DEVENDRA KUMAR")
        );
        assert_eq!(
            find_label_value(&graph, "Pan:").as_deref(),
            Some("AMXPU9247Q")
        );
        assert_eq!(find_label_value(&graph, "Missing:"), None);
    }

    #[test]
    fn test_find_label_values_batch_matches_single() {
        let input = "Invoice Number:|None|None|None|INV-001|10.0|100.0|20.0|105.0\n\
                     Date:|None|None|None|2026-05-23|10.0|90.0|20.0|95.0\n\
                     Total: $500.50|None|None|None|None|10.0|80.0|20.0|85.0\n";
        let graph = parse_flat_graph(input);
        let labels = ["Invoice Number:", "Date:", "Total:", "Missing:"];
        let values = find_label_values(&graph, &labels);
        assert_eq!(values.len(), 4);
        assert_eq!(values[0].as_deref(), Some("INV-001"));
        assert_eq!(values[1].as_deref(), Some("2026-05-23"));
        // Inline span resolved via the fallback path.
        assert_eq!(values[2].as_deref(), Some("$500.50"));
        assert_eq!(values[3], None);
        // Batch results must agree with the single-label API.
        for (label, value) in labels.iter().zip(values.iter()) {
            assert_eq!(value, &find_label_value(&graph, label));
        }
    }

    #[test]
    fn test_find_transaction_rows_page_independent() {
        // Page 1 header at y=500, rows at y=480/470; page 2 header at y=1500, row at y=1480.
        let input = "Date|None|None|None|None|28.0|500.0|45.0|508.0\n\
                     26-Jun-2025|None|None|None|7,999.60|28.0|480.0|65.0|488.0\n\
                     24-Jul-2025|None|None|None|7,999.60|28.0|470.0|65.0|478.0\n\
                     Date|None|None|None|None|28.0|1500.0|45.0|1508.0\n\
                     01-Jan-2026|None|None|None|7,999.60|28.0|1480.0|65.0|1488.0\n";
        let graph = parse_flat_graph(input);
        let rows = find_transaction_rows(&graph, 2.0);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0][0].text, "26-Jun-2025");
        assert_eq!(rows[1][0].text, "24-Jul-2025");
        assert_eq!(rows[2][0].text, "01-Jan-2026");
    }
}
