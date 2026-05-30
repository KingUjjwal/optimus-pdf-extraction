/// A single line from the flat spatial graph.
/// Format: text|top_neighbor|bottom_neighbor|left_neighbor|right_neighbor
#[derive(Debug, Clone)]
pub struct FlatGraphLine {
    pub text: String,
    pub top: String,
    pub bottom: String,
    pub left: String,
    pub right: String,
}

/// Parses the flat spatial graph format into a Vec of lines.
pub fn parse_flat_graph(input: &str) -> Vec<FlatGraphLine> {
    let mut lines = Vec::new();
    for line_str in input.lines() {
        let parts: Vec<&str> = line_str.splitn(5, '|').collect();
        if parts.len() >= 5 {
            lines.push(FlatGraphLine {
                text: parts[0].to_string(),
                top: parts[1].to_string(),
                bottom: parts[2].to_string(),
                left: parts[3].to_string(),
                right: parts[4].to_string(),
            });
        }
    }
    lines
}

/// Finds the right neighbor of a node matching the given anchor text.
pub fn find_right_of(graph: &[FlatGraphLine], anchor: &str) -> Option<String> {
    graph
        .iter()
        .find(|l| l.text == anchor)
        .and_then(|l| {
            if l.right != "None" {
                Some(l.right.clone())
            } else {
                None
            }
        })
}

/// Finds the bottom neighbor of a node matching the given anchor text.
pub fn find_below(graph: &[FlatGraphLine], anchor: &str) -> Option<String> {
    graph
        .iter()
        .find(|l| l.text == anchor)
        .and_then(|l| {
            if l.bottom != "None" {
                Some(l.bottom.clone())
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

/// Guest memory allocator — used by the host to allocate buffer space.
#[no_mangle]
pub extern "C" fn alloc(size: usize) -> *mut u8 {
    let mut buf = Vec::with_capacity(size);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// Guest memory deallocator.
#[no_mangle]
pub extern "C" fn free_buf(ptr: *mut u8, len: usize) {
    if !ptr.is_null() && len > 0 {
        unsafe {
            let _ = Box::from_raw(std::slice::from_raw_parts_mut(ptr, len));
        }
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
        let input = "INVOICE|None|Invoice Number:|None|None\nInvoice Number:|INVOICE|None|None|None\n";
        let graph = parse_flat_graph(input);
        let val = find_below(&graph, "INVOICE");
        assert_eq!(val, Some("Invoice Number:".into()));
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
}
