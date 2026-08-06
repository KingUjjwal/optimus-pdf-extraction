use optimus_core::extract_spans;
use optimus_router::calculate_layout_id;
use std::collections::HashSet;

#[test]
fn test_layout_id_stability() {
    let mut hashes = HashSet::new();
    for i in 0..10 {
        let path = format!("tests/fixtures/invoices/inv_{}.pdf", i);
        let spans = extract_spans(&path).expect("Failed to extract spans from fixture");
        let hash = calculate_layout_id(&spans);

        hashes.insert(hash);
    }

    // Since all 10 PDFs use the exact same template (same layout of anchors),
    // there should only be exactly ONE unique hash.
    assert_eq!(
        hashes.len(),
        1,
        "Layout ID is not stable! Expected 1 hash, found {}",
        hashes.len()
    );
}

#[test]
fn test_layout_id_distinguishes_templates() {
    // A different template with different anchor positions
    let spans1 = optimus_core::extract_spans("../optimus-core/tests/fixtures/report.pdf")
        .expect("Failed to extract report.pdf");

    let hash1 = calculate_layout_id(&spans1);

    // Another template
    let spans2 = optimus_core::extract_spans("../optimus-core/tests/fixtures/form.pdf")
        .expect("Failed to extract form.pdf");

    let hash2 = calculate_layout_id(&spans2);

    assert_ne!(
        hash1, hash2,
        "Different templates must produce different hashes"
    );
}
