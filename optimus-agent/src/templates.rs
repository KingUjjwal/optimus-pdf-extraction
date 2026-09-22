pub const SCHEMA_DISCOVERY_SYSTEM: &str = "\
You are a document schema extractor. Given an ASCII grid representation of a document, \
output ONLY a JSON object describing the key-value pairs that should be extracted. \
Format: {\"field_name\": \"type\"} where type is one of: string, number, date. \
Reply with the JSON object only, no explanation, no markdown fences.";

pub fn schema_discovery_user(grid: &str) -> String {
    format!("Extract a JSON schema from this document:\n\n{}", grid)
}

pub const CODE_GENERATION_SYSTEM: &str = "\
You are a Rust code generator specialized in spatial document extraction for WASM.\
Write a complete cdylib crate with this entry point:

```rust
#[no_mangle]
pub extern \"C\" fn extract(ptr: *const u8, len: usize) -> *mut u8 { ... }
```

The input is a flat spatial graph, one line per span:
  text|top_neighbor|bottom_neighbor|left_neighbor|right_neighbor|x0|y0|x1|y1
The last four coordinate fields are optional (legacy 5-field lines default to 0.0).
Coordinates grow upward with y; multi-page documents offset each page's y.

Rules:
- Use `optimus_guest::*` which provides `parse_flat_graph`, `find_right_of`, `find_left_of`, `find_below`, `find_top_of`, `find_label_value`, `find_label_values`, `find_header`, `rows_below`, `cell_for_header`, `row_to_object`, `find_transaction_rows`, `looks_like_date`, `emit_json`, and `emit_json_typed`.
- Do NOT implement `alloc` and `free_buf` manually, they are provided by `optimus_guest`.
- The entry point MUST be `#[no_mangle] pub extern \"C\" fn extract(ptr: *const u8, len: usize) -> *mut u8`.
- Parse the input `ptr` and `len` into a `&str`, then call `parse_flat_graph` to get a `Vec<FlatGraphLine>`.
- For key-value fields: when there are several scalar fields, call `find_label_values(&graph, &[\"Label A:\", \"Label B:\"])` once (returns a `Vec<Option<String>>` in the same order) — it builds a text index once instead of scanning the graph per field. For a single field use `find_label_value(&graph, \"Label:\")`. Both handle a standalone `Label:` span with a right-neighbor value and an inline `Label: value` span. Fall back to `find_right_of`.
- For transaction/table rows: use `find_transaction_rows(&graph, y_tol)` (anchors rows on DD-MMM-YYYY date cells, page-independent) then `cell_for_header(row, header)` to pick the cell nearest each header's x-center, and build row objects with `row_to_object`.
- Scalars: output JSON using `emit_json(&[(\"field1\", val1), ...])` (all values strings).
- Tables/arrays: use `emit_json_typed(&[(\"field1\", JsonValue::Str(v)), (\"rows\", JsonValue::Array(rows))])` where each row is a `Vec<(String, String)>` of (output_key, value).
- Confidence: when a field came from an exact label match (e.g. `find_label_value` on a known label), use `emit_json_typed_with_confidence(&[(\"field\", JsonValue::Str(v)), ...], &[(\"field\", \"exact\")])`; use `\"heuristic\"` for right-neighbor/below guesses and `\"fuzzy\"` for unfound fields. All three emitters are valid; confidence is optional.
- Return the bytes via `Box::into_raw(bytes.into_boxed_slice()) as *mut u8`.

Return ONLY the complete Rust code, no explanation, no markdown fences.";

pub fn code_generation_user(schema: &str, flat_graph: &str, layout_priors: Option<&str>) -> String {
    match layout_priors {
        Some(priors) if !priors.trim().is_empty() => format!(
            "Target schema: {}\n\n\
             Layout priors (deterministically detected — trust these cell/row hints):\n{}\n\n\
             Flat spatial graph:\n{}",
            schema, priors, flat_graph
        ),
        _ => format!(
            "Target schema: {}\n\nFlat spatial graph:\n{}",
            schema, flat_graph
        ),
    }
}

pub const COMPILATION_FIX_SYSTEM: &str = "\
Fix the compilation errors in the following Rust WASM code.\
The errors come from `rustc --target wasm32-unknown-unknown --crate-type cdylib`.\
Return ONLY the corrected complete Rust code, no explanation, no markdown.";

pub fn compilation_fix_user(rust_code: &str, stderr: &str) -> String {
    format!(
        "Compilation errors:\n{}\n\nRust code:\n{}",
        stderr, rust_code
    )
}

pub const EXTRACTION_FIX_SYSTEM: &str = "\
The Rust WASM module compiled successfully but extracts incorrect values.\
Fix the extraction logic so it correctly navigates the spatial graph.\
Return ONLY the corrected complete Rust code, no explanation, no markdown.";

pub fn extraction_fix_user(rust_code: &str, expected: &str, actual: &str) -> String {
    format!(
        "Expected extraction: {}\nActual extraction: {}\n\nRust code:\n{}",
        expected, actual, rust_code
    )
}

/// Prompt for when the WASM module compiled but crashed/trapped during
/// execution (e.g. missing `extract` export, `unreachable` trap, fuel
/// exhaustion). Includes the actual runtime error so the LLM can repair it.
pub fn extraction_runtime_fix_user(rust_code: &str, expected: &str, error: &str) -> String {
    format!(
        "The WASM module crashed during extraction.\nExpected schema: {}\nRuntime error:\n{}\n\nRust code:\n{}",
        expected, error, rust_code
    )
}
