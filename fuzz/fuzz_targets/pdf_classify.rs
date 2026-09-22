#![no_main]
//! Fuzzes PDF type classification over arbitrary bytes. Exercises lopdf parsing
//! plus the hand-rolled content-stream scanner and the span-based fallback —
//! all of which consume untrusted input.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = optimus_core::detect_pdf_type_bytes(data);
});
