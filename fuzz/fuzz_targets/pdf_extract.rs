#![no_main]
//! Fuzzes the full span-extraction path (pdf_oxide) over arbitrary bytes.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Extraction writes a temp file internally; keep inputs bounded so the
    // fuzzer spends its time parsing rather than in the filesystem.
    if data.len() > 1_000_000 {
        return;
    }
    let _ = optimus_core::extract_spans_from_bytes(data);
});
