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
  text|top_neighbor|bottom_neighbor|left_neighbor|right_neighbor

Rules:
- Use `optimus_guest::*` which provides `parse_flat_graph`, `find_right_of`, `find_below`, and `emit_json`.
- Do NOT implement `alloc` and `free_buf` manually, they are provided by `optimus_guest`.
- The entry point MUST be `#[no_mangle] pub extern \"C\" fn extract(ptr: *const u8, len: usize) -> *mut u8`.
- Parse the input `ptr` and `len` into a `&str`, then call `parse_flat_graph`.
- Find the right neighbor for each field label.
- Output JSON using `emit_json(&[(\"field1\", val1), ...])`.
- Return the bytes via `Box::into_raw(bytes.into_boxed_slice()) as *mut u8`.

The target schema to extract: {schema}
The flat spatial graph of the template document:\n{graph}

Return ONLY the complete Rust code, no explanation, no markdown fences.";

pub fn code_generation_user(schema: &str, flat_graph: &str) -> String {
    CODE_GENERATION_SYSTEM
        .replace("{schema}", schema)
        .replace("{graph}", flat_graph)
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
