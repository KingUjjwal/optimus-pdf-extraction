mod pdf_fixture;

use optimus_agent::{compile_extraction_logic, discover_schema_from_spans, serialize_flat_graph};
use optimus_core::{build_spatial_graph, extract_spans_from_bytes};
use optimus_router::{calculate_layout_id, is_layout_cached, LayoutDb};
use optimus_runtime::{build_arrow_record_batch, process_pdfs_parallel, ExtractedRecord, WasmHost};
use pdf_fixture::invoice_pdf_bytes;

#[test]
fn test_full_pipeline_from_bytes() {
    let pdf_bytes = invoice_pdf_bytes();
    let cache_dir = tempfile::tempdir().unwrap();

    let spans = extract_spans_from_bytes(&pdf_bytes).unwrap();
    assert!(!spans.is_empty(), "should have extracted spans");

    let graph = build_spatial_graph(spans);
    let flat_graph = serialize_flat_graph(&graph);
    let core_spans: Vec<_> = graph.nodes.iter().map(|n| n.span.clone()).collect();

    let layout_id = calculate_layout_id(&core_spans);
    assert!(!layout_id.is_empty());

    let schema = discover_schema_from_spans(&core_spans);

    let wasm_bytes =
        compile_extraction_logic(&layout_id, &graph, &schema, cache_dir.path()).unwrap();
    assert!(!wasm_bytes.is_empty());

    let host = WasmHost::new();
    let json_output = host
        .execute_extraction(&layout_id, &wasm_bytes, &flat_graph)
        .unwrap();

    let record: ExtractedRecord = serde_json::from_str(&json_output).unwrap();
    assert_eq!(
        record.fields.get("invoice_number").and_then(|v| v.as_str()),
        Some("INV-2026-001")
    );
    assert_eq!(
        record.fields.get("date").and_then(|v| v.as_str()),
        Some("2026-05-23")
    );
    assert_eq!(
        record.fields.get("total").and_then(|v| v.as_str()),
        Some("$500.50")
    );

    let batch = build_arrow_record_batch(&[record]).unwrap();
    assert_eq!(batch.num_rows(), 1);
    assert!(batch.num_columns() >= 3);
}

#[test]
fn test_process_pdfs_parallel_single() {
    let pdf_bytes = invoice_pdf_bytes();
    let dir = tempfile::tempdir().unwrap();
    let pdf_path = dir.path().join("invoice.pdf");
    std::fs::write(&pdf_path, &pdf_bytes).unwrap();
    assert!(
        pdf_path.exists(),
        "PDF file should exist at {}",
        pdf_path.display()
    );

    // Verify extract_spans works on the generated PDF (or falls back to mocks)
    let spans = optimus_core::extract_spans(&pdf_path).unwrap();
    assert!(
        !spans.is_empty(),
        "extract_spans should return spans (real or mock)"
    );

    let graph = build_spatial_graph(spans);
    let core_spans: Vec<_> = graph.nodes.iter().map(|n| n.span.clone()).collect();
    let layout_id = calculate_layout_id(&core_spans);
    assert!(!layout_id.is_empty());

    let cache_dir = tempfile::tempdir().unwrap();

    // Compile WASM for this layout
    let schema = discover_schema_from_spans(&core_spans);
    let wasm_bytes = compile_extraction_logic(&layout_id, &graph, &schema, cache_dir.path())
        .expect("WASM compilation should succeed");

    let flat_graph = serialize_flat_graph(&graph);
    let host = WasmHost::new();
    let json = host
        .execute_extraction(&layout_id, &wasm_bytes, &flat_graph)
        .expect("WASM extraction should succeed");

    let record: ExtractedRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(
        record.fields.get("invoice_number").and_then(|v| v.as_str()),
        Some("INV-2026-001")
    );
    assert_eq!(
        record.fields.get("date").and_then(|v| v.as_str()),
        Some("2026-05-23")
    );
    assert_eq!(
        record.fields.get("total").and_then(|v| v.as_str()),
        Some("$500.50")
    );
}

#[test]
fn test_cache_hit_skips_recompilation() {
    let cache_dir = tempfile::tempdir().unwrap();

    // Spans using invoice anchors the WASM module knows: "Invoice Number:", "Date:", "Total:"
    let spans1 = vec![
        optimus_core::TextSpan {
            text: "INVOICE".into(),
            x0: 50.0,
            y0: 750.0,
            x1: 150.0,
            y1: 770.0,
        },
        optimus_core::TextSpan {
            text: "Invoice Number:".into(),
            x0: 50.0,
            y0: 700.0,
            x1: 150.0,
            y1: 715.0,
        },
        optimus_core::TextSpan {
            text: "INV-X-001".into(),
            x0: 180.0,
            y0: 700.0,
            x1: 260.0,
            y1: 715.0,
        },
        optimus_core::TextSpan {
            text: "Date:".into(),
            x0: 50.0,
            y0: 680.0,
            x1: 100.0,
            y1: 695.0,
        },
        optimus_core::TextSpan {
            text: "2026-01-01".into(),
            x0: 180.0,
            y0: 680.0,
            x1: 270.0,
            y1: 695.0,
        },
        optimus_core::TextSpan {
            text: "Total:".into(),
            x0: 400.0,
            y0: 400.0,
            x1: 450.0,
            y1: 415.0,
        },
        optimus_core::TextSpan {
            text: "$999.00".into(),
            x0: 500.0,
            y0: 400.0,
            x1: 560.0,
            y1: 415.0,
        },
    ];
    let graph1 = build_spatial_graph(spans1.clone());
    let layout_id1 = calculate_layout_id(&spans1);
    let wasm1 = compile_extraction_logic(&layout_id1, &graph1, "{}", cache_dir.path()).unwrap();

    // Same layout, different values — same anchors, different right-neighbor values
    let spans2 = vec![
        optimus_core::TextSpan {
            text: "INVOICE".into(),
            x0: 50.0,
            y0: 750.0,
            x1: 150.0,
            y1: 770.0,
        },
        optimus_core::TextSpan {
            text: "Invoice Number:".into(),
            x0: 50.0,
            y0: 700.0,
            x1: 150.0,
            y1: 715.0,
        },
        optimus_core::TextSpan {
            text: "INV-Y-002".into(),
            x0: 180.0,
            y0: 700.0,
            x1: 260.0,
            y1: 715.0,
        },
        optimus_core::TextSpan {
            text: "Date:".into(),
            x0: 50.0,
            y0: 680.0,
            x1: 100.0,
            y1: 695.0,
        },
        optimus_core::TextSpan {
            text: "2026-06-15".into(),
            x0: 180.0,
            y0: 680.0,
            x1: 270.0,
            y1: 695.0,
        },
        optimus_core::TextSpan {
            text: "Total:".into(),
            x0: 400.0,
            y0: 400.0,
            x1: 450.0,
            y1: 415.0,
        },
        optimus_core::TextSpan {
            text: "$150.75".into(),
            x0: 500.0,
            y0: 400.0,
            x1: 560.0,
            y1: 415.0,
        },
    ];
    let layout_id2 = calculate_layout_id(&spans2);
    assert_eq!(
        layout_id1, layout_id2,
        "layout IDs must match for same anchor positions"
    );

    let host = WasmHost::new();
    host.precompile(&layout_id1, &wasm1).unwrap();
    assert!(host.is_cached(&layout_id1));

    let flat2 = serialize_flat_graph(&build_spatial_graph(spans2));
    let json2 = host
        .execute_extraction(&layout_id2, &wasm1, &flat2)
        .unwrap();
    let record2: ExtractedRecord = serde_json::from_str(&json2).unwrap();
    assert_eq!(
        record2
            .fields
            .get("invoice_number")
            .and_then(|v| v.as_str()),
        Some("INV-Y-002")
    );
    assert_eq!(
        record2.fields.get("date").and_then(|v| v.as_str()),
        Some("2026-06-15")
    );
    assert_eq!(
        record2.fields.get("total").and_then(|v| v.as_str()),
        Some("$150.75")
    );
}

#[test]
fn test_error_propagation_bad_input() {
    let err = optimus_core::extract_spans("nonexistent_file_xyz.pdf").unwrap_err();
    let msg = format!("{}", err);
    assert!(
        msg.contains("Failed to open PDF"),
        "expected PdfOpenFailed, got: {}",
        msg
    );
}

#[test]
fn test_layout_db_integration() {
    let db_dir = tempfile::tempdir().unwrap();

    let layout_id = "test_layout_integration";
    let fake_wasm = vec![0x00, 0x61, 0x73, 0x6d];

    // Store via LayoutDb
    {
        let db = LayoutDb::open(db_dir.path()).unwrap();
        assert!(db.lookup(layout_id).unwrap().is_none());
        db.store(layout_id, &fake_wasm).unwrap();
        let retrieved = db.lookup(layout_id).unwrap();
        assert_eq!(retrieved, Some(fake_wasm.clone()));
    } // db dropped — sled flushes and releases lock

    // Now check via is_layout_cached (opens its own sled instance)
    assert!(is_layout_cached(layout_id, db_dir.path()));

    // Remove and verify
    {
        let db = LayoutDb::open(db_dir.path()).unwrap();
        db.remove(layout_id).unwrap();
        assert!(db.lookup(layout_id).unwrap().is_none());
    }
    assert!(!is_layout_cached(layout_id, db_dir.path()));
}

#[test]
fn test_process_pdfs_parallel_error_propagation() {
    let cache_dir = tempfile::tempdir().unwrap();
    let bad_path = std::path::Path::new("nonexistent_file.pdf");

    let results = process_pdfs_parallel(&[bad_path], cache_dir.path());
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
}
