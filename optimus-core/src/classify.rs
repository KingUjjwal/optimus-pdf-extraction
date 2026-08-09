//! Smart PDF type detection without full text extraction.
//!
//! Classifies a PDF as text-based, scanned, image-based, or mixed by sampling
//! content streams for text operators (Tj/TJ) and image invocations (Do)
//! without running the full extraction pipeline. Ported (focused) from
//! firecrawl/pdf-inspector `src/detector.rs` (MIT).
//!
//! Cost: ~10-50ms for a sampled scan — cheap enough to run before the LLM
//! compile loop so scanned documents route to OCR instead of failing.

use crate::text_quality::{
    OCR_REASON_NO_TEXT, OCR_REASON_SCANNED, OCR_REASON_SUSPECTED_GARBLED_TEXT,
    OCR_REASON_VECTOR_TEXT,
};
use lopdf::{Document, Object, ObjectId};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

/// PDF type classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PdfType {
    /// PDF has extractable text (Tj/TJ operators found).
    TextBased,
    /// PDF appears to be scanned (images only, no text operators).
    Scanned,
    /// PDF contains mostly images with minimal/no text.
    ImageBased,
    /// PDF has a mix of text and image-heavy pages.
    Mixed,
}

impl PdfType {
    pub fn as_str(&self) -> &'static str {
        match self {
            PdfType::TextBased => "text",
            PdfType::Scanned => "scanned",
            PdfType::ImageBased => "image",
            PdfType::Mixed => "mixed",
        }
    }
}

/// Strategy for which pages to scan during detection.
#[derive(Debug, Clone)]
pub enum ScanStrategy {
    /// Scan all pages, stop on first non-text page.
    EarlyExit,
    /// Scan all pages, no early exit.
    Full,
    /// Sample up to N evenly distributed pages (first, last, middle).
    Sample(u32),
    /// Only scan these specific 1-indexed page numbers.
    Pages(Vec<u32>),
}

/// Configuration for PDF type detection.
#[derive(Debug, Clone)]
pub struct DetectionConfig {
    pub strategy: ScanStrategy,
    pub min_text_ops_per_page: u32,
    pub text_page_ratio_threshold: f32,
}

impl Default for DetectionConfig {
    fn default() -> Self {
        Self {
            // EarlyExit is too aggressive for PDFs with an image-only cover
            // followed by text-heavy pages (e.g. annual reports).
            strategy: ScanStrategy::Sample(8),
            min_text_ops_per_page: 1,
            text_page_ratio_threshold: 0.6,
        }
    }
}

/// Result of PDF type detection.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PdfTypeResult {
    pub pdf_type: PdfType,
    pub page_count: u32,
    pub pages_sampled: u32,
    pub pages_with_text: u32,
    pub confidence: f32,
    /// True when OCR is recommended for better extraction.
    pub ocr_recommended: bool,
    /// 1-indexed page numbers that need OCR.
    pub pages_needing_ocr: Vec<u32>,
    /// 1-indexed page -> reason codes.
    pub ocr_reasons_by_page: BTreeMap<u32, Vec<String>>,
}

impl Default for PdfTypeResult {
    fn default() -> Self {
        Self {
            pdf_type: PdfType::TextBased,
            page_count: 0,
            pages_sampled: 0,
            pages_with_text: 0,
            confidence: 0.0,
            ocr_recommended: false,
            pages_needing_ocr: Vec::new(),
            ocr_reasons_by_page: BTreeMap::new(),
        }
    }
}

/// Detect PDF type from a file path.
///
/// Tries lopdf first (rich, ~10-50ms). Some real-world PDFs (e.g. ReportLab's
/// `%` comment inside the trailer dict) trip lopdf's strict trailer parser, so
/// on parse failure we fall back to span-based classification via the pdf_oxide
/// pipeline, which is more lenient.
#[tracing::instrument(level = "info", skip_all, fields(path = %path.as_ref().display()))]
pub fn detect_pdf_type<P: AsRef<Path>>(path: P) -> Result<PdfTypeResult, String> {
    match Document::load(&path) {
        Ok(doc) => Ok(detect_from_document(&doc, &DetectionConfig::default())),
        Err(e) => {
            tracing::warn!(
                "lopdf could not parse {:?} ({}); falling back to span-based classification",
                path.as_ref(),
                e
            );
            classify_from_spans_path(&path)
        }
    }
}

/// Detect PDF type from raw bytes (no file system needed).
#[tracing::instrument(level = "info", skip_all)]
pub fn detect_pdf_type_bytes(bytes: &[u8]) -> Result<PdfTypeResult, String> {
    match Document::load_mem(bytes) {
        Ok(doc) => Ok(detect_from_document(&doc, &DetectionConfig::default())),
        Err(e) => {
            tracing::warn!(
                "lopdf could not parse bytes ({}); falling back to span-based classification",
                e
            );
            classify_from_spans_bytes(bytes)
        }
    }
}

/// Detect PDF type with custom configuration.
pub fn detect_pdf_type_with_config<P: AsRef<Path>>(
    path: P,
    config: DetectionConfig,
) -> Result<PdfTypeResult, String> {
    match Document::load(&path) {
        Ok(doc) => Ok(detect_from_document(&doc, &config)),
        Err(e) => {
            tracing::warn!(
                "lopdf could not parse {:?} ({}); falling back to span-based classification",
                path.as_ref(),
                e
            );
            classify_from_spans_path(&path)
        }
    }
}

/// Fallback classification from a file path using the pdf_oxide span pipeline.
fn classify_from_spans_path<P: AsRef<Path>>(path: P) -> Result<PdfTypeResult, String> {
    match crate::extract_spans(path) {
        Ok(spans) => Ok(classify_from_spans(&spans)),
        Err(crate::ExtractionError::NoSpansFound) => Ok(classify_from_spans(&[])),
        Err(e) => Err(format!("extract_spans fallback failed: {}", e)),
    }
}

/// Fallback classification from raw bytes using the pdf_oxide span pipeline.
fn classify_from_spans_bytes(bytes: &[u8]) -> Result<PdfTypeResult, String> {
    match crate::extract_spans_from_bytes(bytes) {
        Ok(spans) => Ok(classify_from_spans(&spans)),
        Err(crate::ExtractionError::NoSpansFound) => Ok(classify_from_spans(&[])),
        Err(e) => Err(format!("extract_spans fallback failed: {}", e)),
    }
}

/// Build a classification result from extracted spans (per-page coverage).
/// Spans without a page number are treated as page 1.
fn classify_from_spans(spans: &[crate::TextSpan]) -> PdfTypeResult {
    if spans.is_empty() {
        return PdfTypeResult {
            pdf_type: PdfType::Scanned,
            page_count: 0,
            pages_sampled: 0,
            pages_with_text: 0,
            confidence: 0.6,
            ocr_recommended: true,
            pages_needing_ocr: vec![1],
            ocr_reasons_by_page: BTreeMap::from([(1, vec![OCR_REASON_SCANNED.to_string()])]),
        };
    }
    let mut pages: BTreeMap<u32, usize> = BTreeMap::new();
    for s in spans {
        let p = s.page.unwrap_or(1);
        *pages.entry(p).or_insert(0) += 1;
    }
    let max_page = pages.keys().copied().max().unwrap_or(1);
    let pages_with_text = pages.len() as u32;
    let pages_needing_ocr: Vec<u32> = (1..=max_page).filter(|p| !pages.contains_key(p)).collect();
    let ocr_reasons_by_page: BTreeMap<u32, Vec<String>> = pages_needing_ocr
        .iter()
        .map(|&p| (p, vec![OCR_REASON_NO_TEXT.to_string()]))
        .collect();
    PdfTypeResult {
        pdf_type: PdfType::TextBased,
        page_count: max_page,
        pages_sampled: max_page,
        pages_with_text,
        confidence: if max_page > 0 {
            pages_with_text as f32 / max_page as f32
        } else {
            0.0
        },
        ocr_recommended: !pages_needing_ocr.is_empty(),
        pages_needing_ocr,
        ocr_reasons_by_page,
    }
}

/// Core classification logic over a loaded document.
pub fn detect_from_document(doc: &Document, config: &DetectionConfig) -> PdfTypeResult {
    let pages = doc.get_pages();
    let total_pages = pages.len() as u32;

    let (sample_indices, allow_early_exit) = match &config.strategy {
        ScanStrategy::EarlyExit => ((1..=total_pages).collect::<Vec<_>>(), true),
        ScanStrategy::Full => ((1..=total_pages).collect::<Vec<_>>(), false),
        ScanStrategy::Sample(max_pages) => {
            let n = (*max_pages).min(total_pages);
            (distribute_pages(n, total_pages), false)
        }
        ScanStrategy::Pages(pages) => {
            let mut valid: Vec<u32> = pages
                .iter()
                .copied()
                .filter(|&p| p >= 1 && p <= total_pages)
                .collect();
            valid.sort_unstable();
            valid.dedup();
            (valid, false)
        }
    };

    let mut pages_with_text = 0u32;
    let mut pages_with_images = 0u32;
    let mut pages_with_template_images = 0u32;
    let mut pages_with_vector_text = 0u32;
    let mut total_text_ops = 0u32;
    let mut analysis_cache: HashMap<u32, PageAnalysis> = HashMap::new();
    let mut pages_actually_sampled = 0u32;

    for page_num in &sample_indices {
        if let Some(&page_id) = pages.get(page_num) {
            let analysis = analyze_page_content(doc, page_id);
            pages_actually_sampled += 1;
            tracing::debug!(
                "classify page {}: text_ops={} images={} template={} unique_chars={} alphanum={} path_ops={} vector={} id_h_no_tounicode={} type3_only={} font_changes={} decodable={}",
                page_num, analysis.text_operator_count, analysis.image_count,
                analysis.has_template_image, analysis.unique_text_chars,
                analysis.unique_alphanum_chars, analysis.path_op_count,
                analysis.has_vector_text, analysis.has_identity_h_no_tounicode,
                analysis.has_only_type3_fonts, analysis.font_change_count,
                analysis.has_decodable_text_fonts
            );
            let is_image_dominated = analysis.image_count > 10
                && analysis.image_count > analysis.text_operator_count * 3;
            let effective_min_ops = if analysis.has_images || analysis.image_count > 0 {
                config.min_text_ops_per_page.max(10)
            } else {
                config.min_text_ops_per_page
            };
            if analysis.text_operator_count >= effective_min_ops
                && !is_image_dominated
                && analysis.unique_text_chars >= 5
                && !analysis.has_vector_text
                && !analysis.has_only_type3_fonts
            {
                pages_with_text += 1;
            }
            if analysis.has_images {
                pages_with_images += 1;
            }
            let alphanum_ok = analysis.unique_alphanum_chars < 10
                && !(analysis.has_decodable_text_fonts && analysis.text_operator_count >= 10);
            if analysis.has_template_image
                && (analysis.image_count <= 1 && analysis.text_operator_count < 50 && alphanum_ok)
            {
                pages_with_template_images += 1;
            }
            if analysis.has_vector_text {
                pages_with_vector_text += 1;
            }
            total_text_ops += analysis.text_operator_count;
            analysis_cache.insert(*page_num, analysis.clone());

            if allow_early_exit
                && (analysis.text_operator_count < config.min_text_ops_per_page
                    || is_image_dominated
                    || analysis.unique_text_chars < 5)
                && (analysis.has_images || analysis.has_template_image)
            {
                break;
            }
        }
    }

    let pages_sampled = pages_actually_sampled;
    let text_ratio = if pages_sampled > 0 {
        pages_with_text as f32 / pages_sampled as f32
    } else {
        0.0
    };

    let has_template_images = pages_with_template_images > 0;
    let template_ratio = if pages_sampled > 0 {
        pages_with_template_images as f32 / pages_sampled as f32
    } else {
        0.0
    };

    let (pdf_type, confidence, ocr_recommended) = if has_template_images && pages_with_text > 0 {
        // Template-based PDF: has text but images provide essential context.
        (PdfType::Mixed, 0.5 + (0.3 * (1.0 - template_ratio)), true)
    } else if text_ratio >= config.text_page_ratio_threshold {
        (PdfType::TextBased, text_ratio, false)
    } else if pages_with_text == 0 && (pages_with_images > 0 || pages_with_vector_text > 0) {
        if total_text_ops == 0 && pages_with_vector_text == 0 {
            (PdfType::Scanned, 0.95, true)
        } else {
            (PdfType::ImageBased, 0.8, true)
        }
    } else if pages_with_text > 0 && (pages_with_images > 0 || pages_with_vector_text > 0) {
        (PdfType::Mixed, 0.7, true)
    } else if total_text_ops == 0 {
        (PdfType::Scanned, 0.9, true)
    } else {
        (PdfType::TextBased, text_ratio.max(0.5), false)
    };

    // Phase 2: build per-page OCR list.
    let mut pages_needing_ocr: Vec<u32> = match pdf_type {
        PdfType::TextBased => Vec::new(),
        PdfType::Scanned | PdfType::ImageBased => (1..=total_pages).collect(),
        PdfType::Mixed => {
            let mut ocr_pages = Vec::new();
            for page_num in 1..=total_pages {
                let analysis = if let Some(cached) = analysis_cache.get(&page_num) {
                    cached.clone()
                } else if let Some(&page_id) = pages.get(&page_num) {
                    let a = analyze_page_content(doc, page_id);
                    analysis_cache.insert(page_num, a.clone());
                    a
                } else {
                    continue;
                };
                let alphanum_low = analysis.unique_alphanum_chars < 10
                    && !(analysis.has_decodable_text_fonts && analysis.text_operator_count >= 10);
                let looks_like_scan =
                    analysis.image_count <= 1 && analysis.text_operator_count < 50 && alphanum_low;
                if (analysis.has_template_image && looks_like_scan)
                    || analysis.has_vector_text
                    || (analysis.text_operator_count < config.min_text_ops_per_page
                        && analysis.has_images)
                {
                    ocr_pages.push(page_num);
                }
            }
            ocr_pages.sort_unstable();
            ocr_pages.dedup();
            ocr_pages
        }
    };

    // Phase 3: flag pages with undecodable fonts.
    for (&page_num, analysis) in &analysis_cache {
        if (analysis.has_identity_h_no_tounicode || analysis.has_only_type3_fonts)
            && !pages_needing_ocr.contains(&page_num)
        {
            pages_needing_ocr.push(page_num);
        }
    }
    pages_needing_ocr.sort_unstable();
    pages_needing_ocr.dedup();

    // Phase 4: explain each OCR-flagged page.
    let mut ocr_reasons_by_page: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    for &page_num in &pages_needing_ocr {
        let reasons = match analysis_cache.get(&page_num) {
            Some(analysis) => page_ocr_reasons(analysis),
            None => vec![OCR_REASON_SCANNED.to_string()],
        };
        ocr_reasons_by_page.insert(page_num, reasons);
    }

    PdfTypeResult {
        pdf_type,
        page_count: total_pages,
        pages_sampled,
        pages_with_text,
        confidence,
        ocr_recommended,
        pages_needing_ocr,
        ocr_reasons_by_page,
    }
}

/// Distribute `n` page indices evenly across `total` pages (1-indexed),
/// always including the first and last page.
fn distribute_pages(n: u32, total: u32) -> Vec<u32> {
    if n == 0 {
        return Vec::new();
    }
    if n >= total {
        return (1..=total).collect();
    }

    let mut indices = Vec::with_capacity(n as usize);
    indices.push(1);
    if n > 1 {
        indices.push(total);
    }

    let remaining = n.saturating_sub(2);
    if remaining > 0 && total > 2 {
        let step = (total - 2) / (remaining + 1);
        for i in 1..=remaining {
            let idx = 1 + (step * i);
            if idx > 1 && idx < total && !indices.contains(&idx) {
                indices.push(idx);
            }
        }
    }

    indices.sort_unstable();
    indices.dedup();
    indices
}

/// Explain why a page needs OCR, from its content analysis.
fn page_ocr_reasons(a: &PageAnalysis) -> Vec<String> {
    let mut reasons = Vec::new();
    if a.has_identity_h_no_tounicode || a.has_only_type3_fonts {
        reasons.push(OCR_REASON_SUSPECTED_GARBLED_TEXT.to_string());
    }
    if a.has_vector_text {
        reasons.push(OCR_REASON_VECTOR_TEXT.to_string());
    }
    if reasons.is_empty() {
        let has_extractable_text = a.text_operator_count > 0 && a.unique_text_chars > 0;
        if !has_extractable_text && !a.has_images && !a.has_template_image {
            reasons.push(OCR_REASON_NO_TEXT.to_string());
        } else {
            reasons.push(OCR_REASON_SCANNED.to_string());
        }
    }
    reasons
}

/// Page content analysis result.
#[derive(Debug, Clone, Default)]
struct PageAnalysis {
    text_operator_count: u32,
    has_images: bool,
    has_template_image: bool,
    image_count: u32,
    unique_text_chars: u32,
    unique_alphanum_chars: u32,
    path_op_count: u32,
    has_vector_text: bool,
    has_identity_h_no_tounicode: bool,
    has_only_type3_fonts: bool,
    font_change_count: u32,
    has_decodable_text_fonts: bool,
}

/// Counts from a single content stream scan.
#[derive(Debug, Default)]
struct ContentCounts {
    text_ops: u32,
    image_count: u32,
    path_ops: u32,
    font_changes: u32,
    unique_chars: HashSet<u8>,
}

const PATH_OPERATORS: &[&[u8]] = &[
    b"m", b"l", b"c", b"v", b"y", b"h", b"re", b"f", b"F", b"S", b"s", b"B", b"B*", b"b", b"b*",
    b"n",
];

fn is_pdf_whitespace(b: u8) -> bool {
    matches!(b, b'\0' | b'\t' | b'\n' | 0x0C | b'\r' | b' ')
}

fn is_pdf_delimiter(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

/// Scans a content stream for text/image/path operators without a full parser.
/// Skips comments, literal strings, hex strings, names, arrays and dicts so
/// operators inside string operands are not miscounted.
fn scan_content(content: &[u8]) -> ContentCounts {
    let mut counts = ContentCounts::default();
    let mut i = 0usize;
    let len = content.len();

    while i < len {
        let b = content[i];
        match b {
            b'%' => {
                while i < len && content[i] != b'\n' {
                    i += 1;
                }
            }
            b'(' => {
                // Literal string: collect unique chars, respect escapes/balance.
                let mut depth = 1i32;
                i += 1;
                while i < len && depth > 0 {
                    let c = content[i];
                    if c == b'\\' {
                        i += 2;
                        continue;
                    }
                    if c == b'(' {
                        depth += 1;
                    } else if c == b')' {
                        depth -= 1;
                    } else {
                        counts.unique_chars.insert(c);
                    }
                    i += 1;
                }
            }
            b'<' => {
                if i + 1 < len && content[i + 1] == b'<' {
                    // Dictionary <<...>>: skip to matching >>.
                    i += 2;
                    let mut depth = 1i32;
                    while i < len && depth > 0 {
                        if content[i] == b'<' && i + 1 < len && content[i + 1] == b'<' {
                            depth += 1;
                            i += 1;
                        } else if content[i] == b'>' && i + 1 < len && content[i + 1] == b'>' {
                            depth -= 1;
                            i += 1;
                        }
                        i += 1;
                    }
                } else {
                    // Hex string <...>.
                    i += 1;
                    while i < len && content[i] != b'>' {
                        counts.unique_chars.insert(content[i]);
                        i += 1;
                    }
                    i += 1; // skip '>'
                }
            }
            b'[' => {
                i += 1;
                let mut depth = 1i32;
                while i < len && depth > 0 {
                    if content[i] == b'[' {
                        depth += 1;
                    } else if content[i] == b']' {
                        depth -= 1;
                    }
                    i += 1;
                }
            }
            b'/' => {
                // Name token.
                i += 1;
                while i < len && !is_pdf_whitespace(content[i]) && !is_pdf_delimiter(content[i]) {
                    i += 1;
                }
            }
            b if is_pdf_whitespace(b) => {
                i += 1;
            }
            _ => {
                // Operator token.
                let start = i;
                while i < len && !is_pdf_whitespace(content[i]) && !is_pdf_delimiter(content[i]) {
                    i += 1;
                }
                if i == start {
                    // Stray delimiter (e.g. unbalanced ')', ']', '>') — skip it.
                    i += 1;
                    continue;
                }
                let token = &content[start..i];
                match token {
                    b"Tj" | b"TJ" => counts.text_ops += 1,
                    b"Tf" => counts.font_changes += 1,
                    b"Do" => counts.image_count += 1,
                    t if PATH_OPERATORS.contains(&t) => counts.path_ops += 1,
                    _ => {}
                }
            }
        }
    }

    counts
}

/// Resolve the (width, height) of a page's MediaBox.
fn page_media_box(doc: &Document, page_id: ObjectId) -> Option<(f32, f32)> {
    let page = doc.get_dictionary(page_id).ok()?;
    let mediabox = page.get(b"MediaBox").ok()?;
    let mediabox = match mediabox {
        Object::Reference(r) => doc.get_object(*r).ok()?,
        other => other,
    };
    let arr = mediabox.as_array().ok()?;
    if arr.len() < 4 {
        return None;
    }
    let (x0, y0, x1, y1) = (
        arr[0].as_f32().ok()?,
        arr[1].as_f32().ok()?,
        arr[2].as_f32().ok()?,
        arr[3].as_f32().ok()?,
    );
    let w = (x1 - x0).abs();
    let h = (y1 - y0).abs();
    (w > 0.0 && h > 0.0).then_some((w, h))
}

/// Check whether a page's resources contain image XObjects and, for large
/// images, whether one covers a template-like fraction of the page.
fn analyze_page_images(doc: &Document, page_id: ObjectId) -> (bool, bool) {
    let (w, h) = page_media_box(doc, page_id).unwrap_or((612.0, 792.0));
    let page_area = w * h;
    let mut found = false;
    let mut has_template = false;

    let (resources, _inherit_chain) = match doc.get_page_resources(page_id) {
        Ok(r) => r,
        Err(_) => return (false, false),
    };
    let Some(resources) = resources else {
        return (false, false);
    };

    let xobjects = match resources.get(b"XObject") {
        Ok(Object::Dictionary(d)) => Some(d.clone()),
        Ok(Object::Reference(r)) => doc.get_dictionary(*r).ok().cloned(),
        _ => None,
    };
    let Some(xobjects) = xobjects else {
        return (found, has_template);
    };

    for (_name, obj) in xobjects.iter() {
        let Object::Reference(r) = obj else {
            continue;
        };
        let Ok(Object::Stream(stream)) = doc.get_object(*r) else {
            continue;
        };
        let is_image = stream
            .dict
            .get(b"Subtype")
            .ok()
            .and_then(|o| o.as_name().ok())
            .map(|s| s == b"Image".as_slice())
            .unwrap_or(false);
        if !is_image {
            continue;
        }
        found = true;
        let img_w = stream
            .dict
            .get(b"Width")
            .ok()
            .and_then(|o| o.as_f32().ok())
            .unwrap_or(0.0);
        let img_h = stream
            .dict
            .get(b"Height")
            .ok()
            .and_then(|o| o.as_f32().ok())
            .unwrap_or(0.0);
        if page_area > 0.0 && img_w * img_h >= page_area * 0.5 {
            has_template = true;
        }
    }

    (found, has_template)
}

/// Analyze a page's content streams and font resources.
fn analyze_page_content(doc: &Document, page_id: ObjectId) -> PageAnalysis {
    let content_streams = doc.get_page_contents(page_id);

    let mut text_ops = 0u32;
    let mut has_images = false;
    let mut image_count = 0u32;
    let mut path_ops = 0u32;
    let mut font_changes = 0u32;
    let mut all_unique_chars: HashSet<u8> = HashSet::new();

    for content_id in content_streams {
        let Ok(Object::Stream(stream)) = doc.get_object(content_id) else {
            continue;
        };
        let content = stream
            .decompressed_content()
            .unwrap_or_else(|_| stream.content.clone());
        let counts = scan_content(&content);
        text_ops += counts.text_ops;
        image_count += counts.image_count;
        path_ops += counts.path_ops;
        font_changes += counts.font_changes;
        all_unique_chars.extend(counts.unique_chars);
        if counts.image_count > 0 {
            has_images = true;
        }
    }

    let (xobj_images, template_image) = analyze_page_images(doc, page_id);
    if xobj_images {
        has_images = true;
        image_count = image_count.max(1);
    }

    let unique_alphanum_chars = all_unique_chars
        .iter()
        .filter(|b| b.is_ascii_alphanumeric())
        .count() as u32;

    let has_vector_text =
        path_ops >= 1000 && path_ops > text_ops.saturating_mul(200) && unique_alphanum_chars < 30;

    let fonts = doc.get_page_fonts(page_id).unwrap_or_default();

    let mut has_identity_h_no_tounicode = false;
    let mut has_only_type3 = false;
    let mut has_type3_with_tounicode = false;
    let mut has_decodable_font = false;

    for font_dict in fonts.values() {
        let subtype = font_dict
            .get(b"Subtype")
            .ok()
            .and_then(|o| o.as_name().ok());
        match subtype {
            Some(b"Type0") => {
                let encoding = font_dict
                    .get(b"Encoding")
                    .ok()
                    .and_then(|o| o.as_name().ok());
                let is_identity = matches!(encoding, Some(b"Identity-H") | Some(b"Identity-V"));
                let has_tounicode = font_dict.get(b"ToUnicode").is_ok();
                if is_identity && !has_tounicode {
                    has_identity_h_no_tounicode = true;
                } else {
                    has_decodable_font = true;
                }
            }
            Some(b"Type3") => {
                if font_dict.get(b"ToUnicode").is_ok() {
                    has_type3_with_tounicode = true;
                    has_decodable_font = true;
                } else {
                    has_only_type3 = true;
                }
            }
            Some(b"Type1") | Some(b"TrueType") | Some(b"MMType1") | Some(b"Type1C") => {
                has_decodable_font = true;
            }
            _ => {}
        }
    }

    let has_only_type3_fonts = text_ops > 0
        && has_only_type3
        && !has_type3_with_tounicode
        && fonts.keys().all(|k| {
            fonts
                .get(k)
                .and_then(|f| f.get(b"Subtype").ok())
                .and_then(|o| o.as_name().ok())
                == Some(b"Type3")
        });

    PageAnalysis {
        text_operator_count: text_ops,
        has_images,
        has_template_image: template_image,
        image_count,
        unique_text_chars: all_unique_chars.len() as u32,
        unique_alphanum_chars,
        path_op_count: path_ops,
        has_vector_text,
        has_identity_h_no_tounicode: text_ops > 0 && has_identity_h_no_tounicode,
        has_only_type3_fonts,
        font_change_count: font_changes,
        has_decodable_text_fonts: text_ops > 0 && has_decodable_font,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{Dictionary, Stream};

    #[test]
    fn scan_counts_text_and_images() {
        // One Tj, one TJ, one Do, a couple of path ops, a literal string.
        let content =
            b"BT /F1 12 Tf 72 712 Td (Invoice Number: INV-001) Tj ET\n0 0 500 700 re f\nq 10 10 100 100 cm /Im1 Do Q\n[ (a) (b) ] TJ\n";
        let c = scan_content(content);
        assert_eq!(c.text_ops, 2);
        assert_eq!(c.image_count, 1);
        assert!(c.path_ops >= 2);
        assert_eq!(c.font_changes, 1);
        assert!(c.unique_chars.contains(&b'I'));
        assert!(c.unique_chars.contains(&b'-'));
    }

    #[test]
    fn operators_inside_strings_not_counted() {
        let content = b"(say Tj here) Tj (Do inside) TJ";
        let c = scan_content(content);
        assert_eq!(c.text_ops, 2);
    }

    #[test]
    fn stray_delimiters_do_not_loop() {
        // Unbalanced ')' ']' '>' must not stall the scanner.
        let content = b") ] > BT (x) Tj ET";
        let c = scan_content(content);
        assert_eq!(c.text_ops, 1);
    }

    #[test]
    fn distribute_sampling_covers_first_and_last() {
        let idx = distribute_pages(4, 100);
        assert_eq!(idx.first(), Some(&1));
        assert_eq!(idx.last(), Some(&100));
        assert_eq!(idx.len(), 4);
    }

    #[test]
    fn synthetic_text_pdf_classifies_textbased() {
        let doc = build_synthetic_document(true);
        let result = detect_from_document(&doc, &DetectionConfig::default());
        assert_eq!(result.pdf_type, PdfType::TextBased);
        assert!(!result.ocr_recommended);
        assert!(result.pages_needing_ocr.is_empty());
    }

    #[test]
    fn synthetic_blank_pdf_has_no_text() {
        let doc = build_synthetic_document(false);
        let result = detect_from_document(&doc, &DetectionConfig::default());
        assert!(result.pages_sampled >= 1);
        assert_eq!(result.pages_with_text, 0);
    }

    #[test]
    fn span_fallback_flags_empty_pages_for_ocr() {
        // Pages 1 and 3 have text; page 2 is a text-less (scanned) insert.
        let spans = vec![
            crate::TextSpan {
                text: "INVOICE".into(),
                x0: 0.0,
                y0: 0.0,
                x1: 10.0,
                y1: 10.0,
                page: Some(1),
                font_size: 0.0,
                is_bold: false,
                is_italic: false,
            },
            crate::TextSpan {
                text: "Total: $50".into(),
                x0: 0.0,
                y0: 20.0,
                x1: 10.0,
                y1: 30.0,
                page: Some(3),
                font_size: 0.0,
                is_bold: false,
                is_italic: false,
            },
        ];
        let r = classify_from_spans(&spans);
        assert_eq!(r.pdf_type, PdfType::TextBased);
        assert!(r.ocr_recommended);
        assert_eq!(r.pages_needing_ocr, vec![2]);
    }

    #[test]
    fn span_fallback_empty_spans_is_scanned() {
        let r = classify_from_spans(&[]);
        assert_eq!(r.pdf_type, PdfType::Scanned);
        assert!(r.ocr_recommended);
    }

    fn build_synthetic_document(with_text: bool) -> Document {
        let mut content = Vec::new();
        if with_text {
            content.extend_from_slice(
                b"BT /F1 12 Tf 72 700 Td (Invoice Number: INV-2026-001) Tj ET\n\
                  BT /F1 12 Tf 72 680 Td (Date: 2026-05-23) Tj ET\n\
                  BT /F1 12 Tf 72 660 Td (Total: $500.50) Tj ET\n",
            );
        }
        let content_id = (1, 0);
        let page_id = (2, 0);
        let catalog_id = (3, 0);

        let mut doc = Document::with_version("1.4");
        let content_obj = Object::Stream(Stream::new(Dictionary::new(), content));
        doc.objects.insert(content_id, content_obj);

        let mut page_dict = Dictionary::new();
        page_dict.set("Type", Object::Name(b"Page".to_vec()));
        page_dict.set(
            "MediaBox",
            Object::Array(vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Integer(612),
                Object::Integer(792),
            ]),
        );
        if with_text {
            page_dict.set("Contents", Object::Reference(content_id));
        }
        doc.objects.insert(page_id, Object::Dictionary(page_dict));

        let mut pages_tree = Dictionary::new();
        pages_tree.set("Type", Object::Name(b"Pages".to_vec()));
        pages_tree.set("Kids", Object::Array(vec![Object::Reference(page_id)]));
        pages_tree.set("Count", Object::Integer(1));
        doc.objects.insert((4, 0), Object::Dictionary(pages_tree));

        let mut catalog = Dictionary::new();
        catalog.set("Type", Object::Name(b"Catalog".to_vec()));
        catalog.set("Pages", Object::Reference((4, 0)));
        doc.objects.insert(catalog_id, Object::Dictionary(catalog));
        doc.trailer.set("Root", Object::Reference(catalog_id));
        doc
    }
}
