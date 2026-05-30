use optimus_guest::*;

#[no_mangle]
pub extern "C" fn extract(ptr: *const u8, len: usize) -> *mut u8 {
    let graph_str = unsafe {
        let slice = std::slice::from_raw_parts(ptr, len);
        match std::str::from_utf8(slice) {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        }
    };

    let graph = parse_flat_graph(graph_str);

    let mut invoice_number = String::from("Unknown");
    let mut date = String::from("Unknown");
    let mut total = String::from("Unknown");

    if let Some(val) = find_right_of(&graph, "Invoice Number:") {
        invoice_number = val;
    }
    if let Some(val) = find_right_of(&graph, "Date:") {
        date = val;
    }
    if let Some(val) = find_right_of(&graph, "Total:") {
        total = val;
    }

    let json_bytes = emit_json(&[
        ("invoice_number", &invoice_number),
        ("date", &date),
        ("total", &total),
    ]);

    let boxed = json_bytes.into_boxed_slice();
    Box::into_raw(boxed) as *mut u8
}
