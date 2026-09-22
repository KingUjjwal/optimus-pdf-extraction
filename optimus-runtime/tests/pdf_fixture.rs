pub fn invoice_pdf_bytes() -> Vec<u8> {
    let header = b"%PDF-1.4\n";

    let obj1 = b"1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n";
    let obj2 = b"2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n";

    let content_stream = b"BT\n\
/F1 12 Tf\n\
50 750 Td (INVOICE) Tj\n\
0 -50 Td (Invoice Number:) Tj\n\
130 0 Td (INV-2026-001) Tj\n\
-130 -20 Td (Date:) Tj\n\
130 0 Td (2026-05-23) Tj\n\
220 -280 Td (Total:) Tj\n\
100 0 Td ($500.50) Tj\n\
ET\n";

    let stream_len = content_stream.len();
    let obj4_header = format!("4 0 obj<</Length {}>>stream\n", stream_len);

    let obj3_template =
        "3 0 obj<</Type/Page/MediaBox[0 0 612 792]/Parent 2 0 R/Resources<</Font<</F1<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>>>>>/Contents 4 0 R>>endobj\n";

    let mut buf = Vec::new();

    buf.extend_from_slice(header);
    let obj1_offset = buf.len();
    buf.extend_from_slice(obj1);
    let obj2_offset = buf.len();
    buf.extend_from_slice(obj2);
    let obj3_offset = buf.len();
    buf.extend_from_slice(obj3_template.as_bytes());
    let obj4_offset = buf.len();
    buf.extend_from_slice(obj4_header.as_bytes());
    buf.extend_from_slice(content_stream);
    buf.extend_from_slice(b"\nendstream\nendobj\n");

    let xref_offset = buf.len();

    let xref = format!(
        "xref\n0 5\n0000000000 65535 f \n{:010} 00000 n \n{:010} 00000 n \n{:010} 00000 n \n{:010} 00000 n \n",
        obj1_offset, obj2_offset, obj3_offset, obj4_offset,
    );

    let trailer = format!(
        "trailer\n<</Size 5/Root 1 0 R>>\nstartxref\n{}\n%%EOF\n",
        xref_offset
    );

    buf.extend_from_slice(xref.as_bytes());
    buf.extend_from_slice(trailer.as_bytes());

    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invoice_pdf_starts_with_header() {
        let pdf = invoice_pdf_bytes();
        assert!(pdf.starts_with(b"%PDF-1.4"));
    }

    #[test]
    fn test_invoice_pdf_contains_text() {
        let pdf = invoice_pdf_bytes();
        let s = String::from_utf8_lossy(&pdf);
        assert!(s.contains("INVOICE"));
        assert!(s.contains("Invoice Number:"));
        assert!(s.contains("INV-2026-001"));
        assert!(s.contains("Total:"));
        assert!(s.contains("$500.50"));
    }

    #[test]
    fn test_invoice_pdf_ends_with_eof() {
        let pdf = invoice_pdf_bytes();
        let s = String::from_utf8_lossy(&pdf);
        assert!(s.ends_with("%%EOF\n"));
    }
}
