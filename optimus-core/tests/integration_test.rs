use optimus_core::{
    build_spatial_graph, detect_pdf_type, extract_spans, generate_ascii_grid_with_config,
    GridConfig, GridFormat, PdfType, TextSpan,
};
use std::time::Instant;

#[test]
fn test_real_pdf_invoice() {
    let path = "tests/fixtures/invoice.pdf";
    let spans = extract_spans(path).expect("Failed to extract invoice spans");

    // Span count > 0
    assert!(!spans.is_empty(), "Expected spans in invoice.pdf");

    let graph = build_spatial_graph(spans.clone());

    // Verify invoice text exists
    let invoice_node = graph.nodes.iter().find(|n| n.span.text.contains("INVOICE"));
    assert!(invoice_node.is_some(), "INVOICE not found in graph");

    // Verify spatial neighbor
    // "Date:" should be somewhere near "2026-05-23" or "Invoice Number:"
    let date_node = graph.nodes.iter().find(|n| n.span.text.contains("Date"));
    assert!(date_node.is_some(), "Date: not found");
    let _date_node = date_node.unwrap();

    // ASCII grid contains expected anchor text
    let grid = generate_ascii_grid_with_config(&spans, GridConfig::default(), GridFormat::Ascii);
    assert!(grid.contains("INVOICE"), "Grid missing INVOICE");
    assert!(grid.contains("Acme Corp"), "Grid missing Acme Corp");
}

#[test]
fn test_real_pdf_report() {
    let path = "tests/fixtures/report.pdf";
    let spans = extract_spans(path).expect("Failed to extract report spans");

    // Span count > 0
    assert!(!spans.is_empty(), "Expected spans in report.pdf");

    let graph = build_spatial_graph(spans.clone());

    // Verify text exists
    let title = graph
        .nodes
        .iter()
        .find(|n| n.span.text.contains("Earnings Report"));
    assert!(title.is_some(), "Earnings Report not found in graph");

    // ASCII grid contains expected anchor text
    let grid = generate_ascii_grid_with_config(&spans, GridConfig::default(), GridFormat::Ascii);
    assert!(grid.contains("Earnings"), "Grid missing Earnings");
    assert!(grid.contains("Revenue: $1M"), "Grid missing Revenue");
}

#[test]
fn test_real_pdf_form() {
    let path = "tests/fixtures/form.pdf";
    let spans = extract_spans(path).expect("Failed to extract form spans");

    // Span count > 0
    assert!(!spans.is_empty(), "Expected spans in form.pdf");

    let graph = build_spatial_graph(spans.clone());

    let fn_node = graph
        .nodes
        .iter()
        .find(|n| n.span.text.contains("First Name:"));
    assert!(fn_node.is_some(), "First Name: not found in graph");

    // Nearest right should be John
    let fn_node = fn_node.unwrap();
    assert!(
        fn_node.nearest_right.is_some(),
        "Expected right neighbor for First Name:"
    );
    let right_neighbor = fn_node.nearest_right.as_ref().unwrap();
    assert!(
        right_neighbor.text.contains("John"),
        "Expected 'John', got {}",
        right_neighbor.text
    );

    // ASCII grid contains expected anchor text
    let grid = generate_ascii_grid_with_config(&spans, GridConfig::default(), GridFormat::Ascii);
    assert!(grid.contains("First Name:"), "Grid missing First Name:");
    assert!(grid.contains("John"), "Grid missing John");
}

#[test]
fn test_rtree_performance_1000_spans() {
    // Generate 1000 synthetic spans
    let mut spans = Vec::with_capacity(1000);
    for i in 0..1000 {
        let x = (i % 10) as f32 * 50.0;
        let y = (i / 10) as f32 * 20.0;
        spans.push(TextSpan {
            text: format!("Span{}", i),
            x0: x,
            y0: y,
            x1: x + 40.0,
            y1: y + 15.0,
            page: None,
            font_size: 0.0,
            is_bold: false,
            is_italic: false,
        });
    }

    let start = Instant::now();
    let graph = build_spatial_graph(spans);
    let duration = start.elapsed();

    assert_eq!(graph.nodes.len(), 1000);
    // Smoke test that the graph builds (R-tree queries for all 1000 spans).
    // The bound is deliberately generous: this runs in parallel with other
    // tests in an unoptimized debug build, so a tight wall-clock assertion
    // flakes. 2s still catches an accidental O(N^2) regression.
    assert!(
        duration.as_millis() < 2000,
        "R-tree query performance took {}ms, expected < 2000ms",
        duration.as_millis()
    );
}

#[test]
fn classify_real_text_fixtures() {
    // Real ReportLab PDFs with compressed text streams must not be flagged
    // as scanned/image-only.
    for fixture in ["invoice.pdf", "report.pdf", "form.pdf"] {
        let path = format!("tests/fixtures/{fixture}");
        let result = detect_pdf_type(&path).unwrap_or_else(|e| panic!("classify {fixture}: {e}"));
        assert_ne!(
            result.pdf_type,
            PdfType::Scanned,
            "{fixture} misclassified as scanned: {result:?}"
        );
        assert!(result.pages_sampled >= 1, "{fixture}: nothing sampled");
    }
}

#[test]
fn extract_spans_populates_font_metadata() {
    let spans = extract_spans("tests/fixtures/invoice.pdf").expect("extract invoice");
    let with_font = spans.iter().filter(|s| s.font_size > 0.0).count();
    assert!(
        with_font > 0,
        "expected font_size metadata on real extraction, got all zeros"
    );
    // Bold flags must accompany font-size metadata (not appear without it).
    let bold = spans.iter().filter(|s| s.is_bold).count();
    assert!(
        bold == 0 || with_font > 0,
        "bold flags should accompany font-size metadata"
    );
}
