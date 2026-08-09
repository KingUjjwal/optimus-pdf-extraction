use optimus_agent::serialize_flat_graph;
use optimus_core::{
    build_spatial_graph, detect_pdf_type, extract_spans_with_quality,
    generate_ascii_grid_with_config, GridConfig, GridFormat,
};
use optimus_router::{calculate_layout_id, is_layout_cached};
use std::path::PathBuf;
use tauri::AppHandle;

use super::emit_event;
use super::types::{Bounds, IngestFullResult, IngestResult};

#[tauri::command]
#[tracing::instrument(level = "info", skip(app))]
pub async fn ingest_pdf_command(path: String, app: AppHandle) -> Result<IngestResult, String> {
    let app_clone = app.clone();
    tokio::task::spawn_blocking(move || {
        let t0 = std::time::Instant::now();
        emit_event(
            &app_clone,
            "pipeline:ingest-start",
            serde_json::json!({"path": path}),
        );

        let (spans, quality) = extract_spans_with_quality(&path)
            .map_err(|e| format!("Failed to extract spans: {}", e))?;

        let count = spans.len();

        emit_event(
            &app_clone,
            "pipeline:ingest-done",
            serde_json::json!({
                "spans": count,
                "spans_data": spans,
                "has_encoding_issues": quality.has_encoding_issues,
                "pages_needing_ocr": quality.pages_needing_ocr,
                "duration_ms": t0.elapsed().as_millis()
            }),
        );

        Ok(IngestResult {
            spans,
            count,
            quality,
        })
    })
    .await
    .map_err(|e| format!("task error: {}", e))?
}

#[tauri::command]
#[tracing::instrument(level = "info", skip(app))]
pub async fn ingest_command(
    path: String,
    cache_dir: String,
    app: AppHandle,
) -> Result<IngestFullResult, String> {
    let app_clone = app.clone();
    tokio::task::spawn_blocking(move || {
        let t0 = std::time::Instant::now();

        emit_event(
            &app_clone,
            "pipeline:ingest-start",
            serde_json::json!({"path": &path}),
        );

        let classification = detect_pdf_type(&path).unwrap_or_default();
        emit_event(
            &app_clone,
            "pipeline:classify-done",
            serde_json::json!({
                "pdf_type": classification.pdf_type.as_str(),
                "ocr_recommended": classification.ocr_recommended,
                "pages_needing_ocr": classification.pages_needing_ocr,
                "confidence": classification.confidence,
            }),
        );

        let needs_ocr = classification.ocr_recommended;
        let (spans, quality) = match extract_spans_with_quality(&path) {
            Ok(r) => r,
            Err(e) if needs_ocr => {
                // Scanned/image-based PDF: no text layer to extract. Route to
                // OCR instead of hard-failing the wizard.
                log::warn!(
                    "extract_spans failed for scanned {} ({}); routing to OCR",
                    path,
                    e
                );
                emit_event(
                    &app_clone,
                    "pipeline:ocr-needed",
                    serde_json::json!({
                        "pdf_type": classification.pdf_type.as_str(),
                        "pages_needing_ocr": classification.pages_needing_ocr,
                        "reasons": classification.ocr_reasons_by_page,
                    }),
                );
                (Vec::new(), Default::default())
            }
            Err(e) => return Err(format!("extract_spans: {}", e)),
        };

        emit_event(
            &app_clone,
            "pipeline:ingest-done",
            serde_json::json!({
                "spans": spans.len(),
                "spans_data": spans,
                "has_encoding_issues": quality.has_encoding_issues,
                "pages_needing_ocr": quality.pages_needing_ocr,
                "duration_ms": t0.elapsed().as_millis()
            }),
        );

        let graph = build_spatial_graph(spans.clone());
        let flat_graph = serialize_flat_graph(&graph);
        let core_spans: Vec<_> = graph.nodes.iter().map(|n| n.span.clone()).collect();

        emit_event(
            &app_clone,
            "pipeline:graph-built",
            serde_json::json!({
                "nodes": graph.nodes.len(),
                "duration_ms": t0.elapsed().as_millis()
            }),
        );

        let layout_id = calculate_layout_id(&core_spans);
        let grid = generate_ascii_grid_with_config(
            &core_spans,
            GridConfig {
                include_font_size: true,
                ..GridConfig::default()
            },
            GridFormat::Ascii,
        );

        let mut min_x = f32::MAX;
        let mut max_x = -f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_y = -f32::MAX;
        for s in &core_spans {
            if s.x0 < min_x {
                min_x = s.x0;
            }
            if s.x1 > max_x {
                max_x = s.x1;
            }
            if s.y0 < min_y {
                min_y = s.y0;
            }
            if s.y1 > max_y {
                max_y = s.y1;
            }
        }

        let cache_path = PathBuf::from(&cache_dir);
        if let Err(e) = std::fs::create_dir_all(&cache_path) {
            log::warn!("Failed to create directory {:?}: {}", cache_path, e);
        }
        let is_cached = is_layout_cached(&layout_id, &cache_path);

        emit_event(
            &app_clone,
            "pipeline:layout-hash",
            serde_json::json!({
                "layout_id": &layout_id,
                "is_cached": is_cached,
                "duration_ms": t0.elapsed().as_millis()
            }),
        );

        Ok(IngestFullResult {
            count: spans.len(),
            spans,
            grid,
            flat_graph,
            layout_id,
            is_cached,
            bounding_box: Bounds {
                min_x,
                max_x,
                min_y,
                max_y,
            },
            quality,
            classification,
            needs_ocr,
        })
    })
    .await
    .map_err(|e| format!("task error: {}", e))?
}
