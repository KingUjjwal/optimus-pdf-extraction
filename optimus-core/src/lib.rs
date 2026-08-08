use memmap2::Mmap;
use rstar::{RTree, RTreeObject, AABB};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::File;
use std::path::Path;

pub mod text_quality;

pub use text_quality::{
    analyze_text_quality, detect_encoding_issues, is_cid_garbage, is_garbage_text,
    span_has_strong_issue, TextQualityReport, OCR_REASON_NO_TEXT, OCR_REASON_SCANNED,
    OCR_REASON_SUSPECTED_GARBLED_TEXT, OCR_REASON_VECTOR_TEXT,
};

#[derive(Debug)]
pub enum ExtractionError {
    PdfOpenFailed(std::io::Error),
    PdfParseFailed(String),
    NoSpansFound,
    TempFileError(std::io::Error),
    MmapError(std::io::Error),
}

impl fmt::Display for ExtractionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExtractionError::PdfOpenFailed(e) => write!(f, "Failed to open PDF: {}", e),
            ExtractionError::PdfParseFailed(s) => write!(f, "Failed to parse PDF: {}", s),
            ExtractionError::NoSpansFound => write!(f, "No text spans found in document"),
            ExtractionError::TempFileError(e) => write!(f, "Temp file error: {}", e),
            ExtractionError::MmapError(e) => write!(f, "Memory-map error: {}", e),
        }
    }
}

impl std::error::Error for ExtractionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ExtractionError::PdfOpenFailed(e) => Some(e),
            ExtractionError::TempFileError(e) => Some(e),
            ExtractionError::MmapError(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ExtractionError {
    fn from(e: std::io::Error) -> Self {
        ExtractionError::PdfOpenFailed(e)
    }
}

pub type Result<T> = std::result::Result<T, ExtractionError>;

/// A single text span with bounding box coordinates.
/// `page` is 1-indexed and absent (None) for synthetic/graph-built spans.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TextSpan {
    pub text: String,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    #[serde(default)]
    pub page: Option<u32>,
}

/// A reference to a neighboring node with distance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpatialNeighbor {
    pub index: usize,
    pub text: String,
    pub distance: f32,
}

/// A node in the spatial graph with cardinal neighbors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpatialNode {
    pub span: TextSpan,
    pub nearest_top: Option<SpatialNeighbor>,
    pub nearest_bottom: Option<SpatialNeighbor>,
    pub nearest_left: Option<SpatialNeighbor>,
    pub nearest_right: Option<SpatialNeighbor>,
}

/// A spatial graph composed of SpatialNode elements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpatialGraph {
    pub nodes: Vec<SpatialNode>,
}

/// Wrapper enabling TextSpan insertion into an R-Tree.
#[derive(Debug, Clone)]
pub struct RTreeSpan {
    pub index: usize,
    pub envelope: AABB<[f32; 2]>,
}

impl RTreeObject for RTreeSpan {
    type Envelope = AABB<[f32; 2]>;
    fn envelope(&self) -> Self::Envelope {
        self.envelope
    }
}

/// Ingests a PDF file using memory-mapping
#[tracing::instrument(level = "debug", skip_all)]
pub fn ingest_pdf<P: AsRef<Path>>(path: P) -> Result<Mmap> {
    let file = File::open(path).map_err(ExtractionError::PdfOpenFailed)?;
    let mmap = unsafe { Mmap::map(&file).map_err(ExtractionError::MmapError)? };
    Ok(mmap)
}

/// Extracts text spans from a PDF file.
/// Uses memmap2 to memory-map the file, then uses pdf_oxide to extract text spans.
/// Falls back to mock spans with a logged warning when pdf_oxide fails or returns empty.
#[tracing::instrument(level = "info", skip_all, fields(path = %path.as_ref().display()))]
pub fn extract_spans<P: AsRef<Path>>(path: P) -> Result<Vec<TextSpan>> {
    let file = File::open(&path).map_err(ExtractionError::PdfOpenFailed)?;
    let _mmap = unsafe { Mmap::map(&file).map_err(ExtractionError::MmapError)? };

    match pdf_oxide::PdfDocument::open(&path) {
        Ok(mut doc) => {
            let mut spans = Vec::new();
            let page_count = doc
                .page_count()
                .map_err(|e| ExtractionError::PdfParseFailed(e.to_string()))?;
            let mut page_offset_y = 0.0f32;
            for page_num in 0..page_count {
                if let Ok(raw_spans) = doc.extract_spans(page_num) {
                    let mut page_top = f32::MAX;
                    let mut page_bottom = 0.0f32;
                    for s in &raw_spans {
                        let top = s.bbox.top();
                        let bottom = s.bbox.bottom();
                        if top < page_top {
                            page_top = top;
                        }
                        if bottom > page_bottom {
                            page_bottom = bottom;
                        }
                    }
                    let page_h = if page_top < f32::MAX {
                        page_bottom - page_top
                    } else {
                        0.0
                    };
                    for s in &raw_spans {
                        spans.push(TextSpan {
                            text: s.text.clone(),
                            x0: s.bbox.left(),
                            y0: s.bbox.top() + page_offset_y,
                            x1: s.bbox.right(),
                            y1: s.bbox.bottom() + page_offset_y,
                            page: Some((page_num + 1) as u32),
                        });
                    }
                    if page_h > 0.0 {
                        page_offset_y += page_h + 50.0;
                    }
                }
            }
            if spans.is_empty() {
                log::warn!("pdf_oxide returned 0 spans for {:?}", path.as_ref());
                #[cfg(test)]
                return Ok(get_mock_spans());
                #[cfg(not(test))]
                return Err(ExtractionError::NoSpansFound);
            }
            Ok(spans)
        }
        Err(e) => {
            log::warn!("pdf_oxide failed to open {:?}: {}", path.as_ref(), e);
            #[cfg(test)]
            return Ok(get_mock_spans());
            #[cfg(not(test))]
            return Err(ExtractionError::PdfParseFailed(e.to_string()));
        }
    }
}

/// Extracts text spans from a byte slice. Falls back to mock spans.
#[tracing::instrument(level = "debug", skip_all)]
pub fn extract_spans_from_bytes(pdf_bytes: &[u8]) -> Result<Vec<TextSpan>> {
    let dir = tempfile::tempdir().map_err(ExtractionError::TempFileError)?;
    let path = dir.path().join("document.pdf");
    std::fs::write(&path, pdf_bytes).map_err(ExtractionError::TempFileError)?;
    extract_spans(&path)
}

/// Extracts text spans plus a per-page text-quality report.
///
/// Runs `extract_spans`, then `analyze_text_quality` over the result so
/// callers can detect garbled/mojibake text layers before schema inference.
#[tracing::instrument(level = "info", skip_all, fields(path = %path.as_ref().display()))]
pub fn extract_spans_with_quality<P: AsRef<Path>>(
    path: P,
) -> Result<(Vec<TextSpan>, TextQualityReport)> {
    let spans = extract_spans(&path)?;
    let report = analyze_text_quality(&spans);
    if report.has_encoding_issues {
        log::warn!(
            "text-quality: {} page(s) flagged for OCR on {:?}: {:?}",
            report.pages_needing_ocr.len(),
            path.as_ref(),
            report.reasons_by_page,
        );
    }
    Ok((spans, report))
}

#[cfg(test)]
fn get_mock_spans() -> Vec<TextSpan> {
    vec![
        TextSpan {
            text: "INVOICE".to_string(),
            x0: 50.0,
            y0: 750.0,
            x1: 150.0,
            y1: 770.0,
            page: None,
        },
        TextSpan {
            text: "Invoice Number:".to_string(),
            x0: 50.0,
            y0: 700.0,
            x1: 150.0,
            y1: 715.0,
            page: None,
        },
        TextSpan {
            text: "INV-2026-001".to_string(),
            x0: 180.0,
            y0: 700.0,
            x1: 280.0,
            y1: 715.0,
            page: None,
        },
        TextSpan {
            text: "Date:".to_string(),
            x0: 50.0,
            y0: 680.0,
            x1: 100.0,
            y1: 695.0,
            page: None,
        },
        TextSpan {
            text: "2026-05-23".to_string(),
            x0: 180.0,
            y0: 680.0,
            x1: 270.0,
            y1: 695.0,
            page: None,
        },
        TextSpan {
            text: "Bill To:".to_string(),
            x0: 50.0,
            y0: 630.0,
            x1: 100.0,
            y1: 645.0,
            page: None,
        },
        TextSpan {
            text: "Acme Corp".to_string(),
            x0: 50.0,
            y0: 610.0,
            x1: 120.0,
            y1: 625.0,
            page: None,
        },
        TextSpan {
            text: "Description".to_string(),
            x0: 50.0,
            y0: 530.0,
            x1: 150.0,
            y1: 545.0,
            page: None,
        },
        TextSpan {
            text: "Quantity".to_string(),
            x0: 300.0,
            y0: 530.0,
            x1: 350.0,
            y1: 545.0,
            page: None,
        },
        TextSpan {
            text: "Unit Price".to_string(),
            x0: 400.0,
            y0: 530.0,
            x1: 460.0,
            y1: 545.0,
            page: None,
        },
        TextSpan {
            text: "Amount".to_string(),
            x0: 500.0,
            y0: 530.0,
            x1: 550.0,
            y1: 545.0,
            page: None,
        },
        TextSpan {
            text: "Cloud Database Hosting".to_string(),
            x0: 50.0,
            y0: 500.0,
            x1: 200.0,
            y1: 515.0,
            page: None,
        },
        TextSpan {
            text: "1".to_string(),
            x0: 300.0,
            y0: 500.0,
            x1: 310.0,
            y1: 515.0,
            page: None,
        },
        TextSpan {
            text: "$500.00".to_string(),
            x0: 400.0,
            y0: 500.0,
            x1: 450.0,
            y1: 515.0,
            page: None,
        },
        TextSpan {
            text: "$500.00".to_string(),
            x0: 500.0,
            y0: 500.0,
            x1: 550.0,
            y1: 515.0,
            page: None,
        },
        TextSpan {
            text: "Server Serverless Compute".to_string(),
            x0: 50.0,
            y0: 480.0,
            x1: 220.0,
            y1: 495.0,
            page: None,
        },
        TextSpan {
            text: "10".to_string(),
            x0: 300.0,
            y0: 480.0,
            x1: 315.0,
            y1: 495.0,
            page: None,
        },
        TextSpan {
            text: "$0.05".to_string(),
            x0: 400.0,
            y0: 480.0,
            x1: 430.0,
            y1: 495.0,
            page: None,
        },
        TextSpan {
            text: "$0.50".to_string(),
            x0: 500.0,
            y0: 480.0,
            x1: 530.0,
            y1: 495.0,
            page: None,
        },
        TextSpan {
            text: "Total:".to_string(),
            x0: 400.0,
            y0: 400.0,
            x1: 450.0,
            y1: 415.0,
            page: None,
        },
        TextSpan {
            text: "$500.50".to_string(),
            x0: 500.0,
            y0: 400.0,
            x1: 555.0,
            y1: 415.0,
            page: None,
        },
    ]
}

/// Builds the Spatial Graph using R-Tree for O(N log N) nearest-neighbor queries
#[tracing::instrument(level = "debug", skip_all, fields(span_count = spans.len()))]
pub fn build_spatial_graph(spans: Vec<TextSpan>) -> SpatialGraph {
    let mut rtree_entries = Vec::new();
    for (i, span) in spans.iter().enumerate() {
        let envelope = AABB::from_corners([span.x0, span.y0], [span.x1, span.y1]);
        rtree_entries.push(RTreeSpan { index: i, envelope });
    }

    let rtree = RTree::bulk_load(rtree_entries);
    let mut nodes = Vec::new();

    let tolerance = 5.0;

    fn find_nearest_in_corridor(
        spans: &[TextSpan],
        rtree: &RTree<RTreeSpan>,
        src_idx: usize,
        cx: f32,
        cy: f32,
        envelope_min: [f32; 2],
        envelope_max: [f32; 2],
    ) -> Option<SpatialNeighbor> {
        let envelope = AABB::from_corners(envelope_min, envelope_max);
        let candidates = rtree.locate_in_envelope(&envelope);
        let mut best: Option<SpatialNeighbor> = None;
        let mut min_dist = f32::MAX;
        for cand in candidates {
            if cand.index == src_idx {
                continue;
            }
            let o_span = &spans[cand.index];
            let o_cx = (o_span.x0 + o_span.x1) / 2.0;
            let o_cy = (o_span.y0 + o_span.y1) / 2.0;
            let dist = ((o_cx - cx).powi(2) + (o_cy - cy).powi(2)).sqrt();
            if dist < min_dist {
                min_dist = dist;
                best = Some(SpatialNeighbor {
                    index: cand.index,
                    text: o_span.text.clone(),
                    distance: dist,
                });
            }
        }
        best
    }

    for (i, span) in spans.iter().enumerate() {
        let cx = (span.x0 + span.x1) / 2.0;
        let cy = (span.y0 + span.y1) / 2.0;

        let nearest_top = find_nearest_in_corridor(
            &spans,
            &rtree,
            i,
            cx,
            cy,
            [span.x0 - tolerance, span.y1],
            [span.x1 + tolerance, f32::MAX],
        );
        let nearest_bottom = find_nearest_in_corridor(
            &spans,
            &rtree,
            i,
            cx,
            cy,
            [span.x0 - tolerance, -f32::MAX],
            [span.x1 + tolerance, span.y0],
        );
        let nearest_left = find_nearest_in_corridor(
            &spans,
            &rtree,
            i,
            cx,
            cy,
            [-f32::MAX, span.y0 - tolerance],
            [span.x0, span.y1 + tolerance],
        );
        let nearest_right = find_nearest_in_corridor(
            &spans,
            &rtree,
            i,
            cx,
            cy,
            [span.x1, span.y0 - tolerance],
            [f32::MAX, span.y1 + tolerance],
        );

        nodes.push(SpatialNode {
            span: span.clone(),
            nearest_top,
            nearest_bottom,
            nearest_left,
            nearest_right,
        });
    }

    SpatialGraph { nodes }
}

/// Configuration for ASCII/Markdown grid generation.
/// Buckets define how many pixels each grid cell represents.
#[derive(Debug, Clone, Copy)]
pub struct GridConfig {
    pub x_bucket: u32,
    pub y_bucket: u32,
}

impl Default for GridConfig {
    fn default() -> Self {
        Self {
            x_bucket: 8,
            y_bucket: 15,
        }
    }
}

/// Output format for the document grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridFormat {
    Ascii,
    MarkdownTable,
}

/// Generates a lightweight 2D ASCII/Markdown grid of the document.
/// Rounds span coordinates to the nearest bucket pixels and formats it into lines of text.
#[tracing::instrument(level = "debug", skip(spans), fields(span_count = spans.len()))]
pub fn generate_ascii_grid_with_config(
    spans: &[TextSpan],
    config: GridConfig,
    format: GridFormat,
) -> String {
    if spans.is_empty() {
        return String::new();
    }

    let mut min_x = f32::MAX;
    let mut max_x = -f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_y = -f32::MAX;

    for span in spans {
        if span.x0 < min_x {
            min_x = span.x0;
        }
        if span.x1 > max_x {
            max_x = span.x1;
        }
        if span.y0 < min_y {
            min_y = span.y0;
        }
        if span.y1 > max_y {
            max_y = span.y1;
        }
    }

    let char_width = config.x_bucket as f32;
    let char_height = config.y_bucket as f32;

    let cols = ((max_x - min_x) / char_width).ceil() as usize + 1;
    let rows = ((max_y - min_y) / char_height).ceil() as usize + 1;

    let mut grid = vec![vec![' '; cols]; rows];

    for span in spans {
        let col = ((span.x0 - min_x) / char_width).floor() as isize;
        let row = ((max_y - span.y1) / char_height).floor() as isize;

        if col >= 0 && (col as usize) < cols && row >= 0 && (row as usize) < rows {
            let r = row as usize;
            let c = col as usize;
            let text_chars: Vec<char> = span.text.chars().collect();
            for (idx, &ch) in text_chars.iter().enumerate() {
                let target = c + idx;
                if target < cols {
                    grid[r][target] = ch;
                }
            }
        }
    }

    match format {
        GridFormat::Ascii => render_ascii_grid(grid),
        GridFormat::MarkdownTable => render_markdown_table(grid),
    }
}

fn render_ascii_grid(grid: Vec<Vec<char>>) -> String {
    let mut result = String::new();
    for row in grid {
        let line: String = row.into_iter().collect();
        let trimmed = line.trim_end();
        if !trimmed.is_empty() {
            result.push_str(trimmed);
            result.push('\n');
        }
    }
    result
}

fn render_markdown_table(grid: Vec<Vec<char>>) -> String {
    if grid.is_empty() {
        return String::new();
    }
    let cols = grid.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut result = String::new();

    result.push('|');
    for _ in 0..cols {
        result.push_str("   |");
    }
    result.push('\n');

    result.push('|');
    for _ in 0..cols {
        result.push_str("---|");
    }
    result.push('\n');

    for row in grid {
        result.push('|');
        for &ch in row.iter() {
            // Markdown tables need pipe escaping
            let escaped = if ch == '|' { "\\|" } else { &ch.to_string() };
            result.push_str(&format!(" {} |", escaped));
        }
        result.push('\n');
    }
    result
}

/// Generates a lightweight 2D ASCII grid with default bucket sizes (8x15).
/// Kept for backward compatibility — the original Phase 1 interface.
#[tracing::instrument(level = "debug", skip_all)]
pub fn generate_ascii_grid(spans: &[TextSpan]) -> String {
    generate_ascii_grid_with_config(spans, GridConfig::default(), GridFormat::Ascii)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_ingestion_and_graph() {
        let spans = get_mock_spans();
        assert!(!spans.is_empty());

        let graph = build_spatial_graph(spans.clone());
        assert_eq!(graph.nodes.len(), spans.len());

        // Verify invoice header node
        let invoice_node = graph
            .nodes
            .iter()
            .find(|n| n.span.text == "INVOICE")
            .unwrap();
        // Since invoice header is at top-left, bottom nearest neighbor should exist
        assert!(invoice_node.nearest_bottom.is_some());

        // Verify R-Tree search nearest bottom is indeed the invoice number label
        let bot_neigh = invoice_node.nearest_bottom.as_ref().unwrap();
        assert_eq!(bot_neigh.text, "Invoice Number:");

        // Verify ascii grid generation doesn't crash and has text in it
        let grid = generate_ascii_grid(&spans);
        assert!(grid.contains("INVOICE"));
        assert!(grid.contains("Invoice Number:"));
        assert!(grid.contains("Total:"));
    }

    #[test]
    fn test_grid_config_custom_buckets() {
        let spans = get_mock_spans();
        let config = GridConfig {
            x_bucket: 10,
            y_bucket: 20,
        };
        let grid = generate_ascii_grid_with_config(&spans, config, GridFormat::Ascii);
        assert!(!grid.is_empty());
        assert!(grid.contains("INVOICE"));
    }

    #[test]
    fn test_grid_format_markdown_table() {
        let spans = get_mock_spans();
        let grid = generate_ascii_grid_with_config(
            &spans,
            GridConfig::default(),
            GridFormat::MarkdownTable,
        );
        assert!(grid.starts_with('|'));
        assert!(grid.contains("---"));
        // Markdown table splits text into per-char cells: | I | N | V | ...
        assert!(grid.contains('I'));
        assert!(grid.contains('N'));
        assert!(grid.contains('V'));
    }

    #[test]
    fn test_empty_spans_grid() {
        let grid = generate_ascii_grid(&[]);
        assert!(grid.is_empty());
    }

    #[test]
    fn test_extraction_error_display() {
        let err = ExtractionError::NoSpansFound;
        assert_eq!(format!("{}", err), "No text spans found in document");

        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = ExtractionError::PdfOpenFailed(io_err);
        assert!(format!("{}", err).contains("Failed to open PDF"));
    }
}
