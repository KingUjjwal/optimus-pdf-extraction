use optimus_core::TextSpan;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const MAX_ANCHORS: usize = 5;

/// A detected layout anchor with position and page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Anchor {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub page: usize,
}

/// Configurable anchor detector with keywords and regex patterns.
#[derive(Debug, Clone)]
pub struct AnchorDetector {
    pub min_confidence: f32,
    pub custom_keywords: Vec<String>,
    pub custom_patterns: Vec<regex::Regex>,
}

impl Default for AnchorDetector {
    fn default() -> Self {
        Self {
            min_confidence: 0.5,
            custom_keywords: vec![
                "invoice".into(),
                "total".into(),
                "date".into(),
                "bill to".into(),
                "ship to".into(),
                "amount".into(),
                "quantity".into(),
                "unit price".into(),
                "description".into(),
            ],
            custom_patterns: vec![],
        }
    }
}

impl AnchorDetector {
    #[tracing::instrument]
    pub fn new() -> Self {
        Self::default()
    }

    #[tracing::instrument(skip_all)]
    pub fn with_keywords(keywords: Vec<String>) -> Self {
        let mut base = Self::default();
        base.custom_keywords.extend(keywords);
        base
    }

    #[tracing::instrument(skip_all)]
    pub fn with_patterns(patterns: Vec<String>) -> Self {
        let compiled = patterns
            .into_iter()
            .filter_map(|p| regex::Regex::new(&p).ok())
            .collect();
        Self {
            custom_patterns: compiled,
            ..Self::default()
        }
    }
}

/// Identifies if a given text span acts as a potential static layout anchor.
/// Flags elements ending in colons, entirely uppercase letters, or matching common keywords.
#[tracing::instrument(skip_all)]
pub fn is_potential_anchor(text: &str) -> bool {
    detect_anchor(&AnchorDetector::default(), text)
}

/// Detects anchors using a custom detector configuration.
#[tracing::instrument(skip_all)]
pub fn detect_anchor(detector: &AnchorDetector, text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }

    // 1. Ends with a colon (e.g. "Invoice Number:")
    if trimmed.ends_with(':') {
        return true;
    }

    // 2. Entirely uppercase (e.g. "INVOICE") - must not contain digits to exclude dynamic codes
    let has_letters = trimmed.chars().any(|c| c.is_alphabetic());
    let all_upper = trimmed
        .chars()
        .all(|c| !c.is_alphabetic() || c.is_uppercase());
    let has_digits = trimmed.chars().any(|c| c.is_numeric());
    if has_letters && all_upper && !has_digits {
        return true;
    }

    // 3. Regex Patterns
    for pattern in &detector.custom_patterns {
        if pattern.is_match(trimmed) {
            return true;
        }
    }

    // 4. Common headers (word-boundary case-insensitive check)
    let lower = trimmed.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    for kw in &detector.custom_keywords {
        let kw_lower = kw.to_lowercase();
        let kw_parts: Vec<&str> = kw_lower.split_whitespace().collect();
        if kw_parts.len() == 1 {
            if words.contains(&kw_parts[0]) {
                return true;
            }
        } else if lower.contains(&kw_lower) {
            return true;
        }
    }

    false
}

/// Extracts sorted anchors on the first page, sorted top-to-bottom and left-to-right.
#[tracing::instrument(skip_all)]
pub fn extract_anchors(spans: &[TextSpan]) -> Vec<Anchor> {
    extract_anchors_with_detector_page(spans, &AnchorDetector::default(), 0)
}

/// Extracts sorted anchors using a custom detector for a specific page.
#[tracing::instrument(skip_all)]
pub fn extract_anchors_with_detector_page(
    spans: &[TextSpan],
    detector: &AnchorDetector,
    page: usize,
) -> Vec<Anchor> {
    let mut anchors = Vec::new();
    for s in spans {
        if detect_anchor(detector, &s.text) {
            anchors.push(Anchor {
                text: s.text.clone(),
                x: (s.x0 + s.x1) / 2.0,
                y: (s.y0 + s.y1) / 2.0,
                page,
            });
        }
    }

    anchors.sort_by(|a, b| {
        a.page
            .cmp(&b.page)
            .then_with(|| b.y.partial_cmp(&a.y).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| a.text.cmp(&b.text))
    });

    anchors
}

/// Extracts sorted anchors using a custom detector for the first page (backward compat).
#[tracing::instrument(skip_all)]
pub fn extract_anchors_with_detector(spans: &[TextSpan], detector: &AnchorDetector) -> Vec<Anchor> {
    extract_anchors_with_detector_page(spans, detector, 0)
}

/// Extracts anchors across multiple pages.
#[tracing::instrument(skip_all)]
pub fn extract_anchors_multi_page(spans_by_page: &[Vec<TextSpan>]) -> Vec<Anchor> {
    let mut anchors = Vec::new();
    for (page, spans) in spans_by_page.iter().enumerate() {
        anchors.extend(extract_anchors_with_detector_page(
            spans,
            &AnchorDetector::default(),
            page,
        ));
    }
    anchors.sort_by(|a, b| {
        a.page
            .cmp(&b.page)
            .then_with(|| b.y.partial_cmp(&a.y).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| a.text.cmp(&b.text))
    });
    anchors
}

/// Extracts anchors from only the first page for performance short-circuit.
#[tracing::instrument(skip_all)]
pub fn extract_anchors_first_page(spans_by_page: &[Vec<TextSpan>]) -> Vec<Anchor> {
    if let Some(first_page_spans) = spans_by_page.first() {
        extract_anchors(first_page_spans)
    } else {
        Vec::new()
    }
}

/// Computes the relative distance vector (dx, dy) between top N anchors.
/// This is the layout-invariant fingerprint used by calculate_layout_id.
#[tracing::instrument(skip_all)]
pub fn compute_anchor_distances(spans: &[TextSpan]) -> Vec<(f32, f32)> {
    let anchors = extract_anchors(spans);
    let top_anchors = if anchors.len() > MAX_ANCHORS {
        &anchors[..MAX_ANCHORS]
    } else {
        &anchors[..]
    };

    let mut distances = Vec::with_capacity(top_anchors.len().saturating_sub(1));
    for i in 0..top_anchors.len().saturating_sub(1) {
        let dx = top_anchors[i + 1].x - top_anchors[i].x;
        let dy = top_anchors[i + 1].y - top_anchors[i].y;
        distances.push((dx, dy));
    }
    distances
}

/// Generates a cryptographic BLAKE3 Layout_ID hash representing the document layout.
/// Computes the invariant relative distance (dx, dy) vector between the top N static anchors.
#[tracing::instrument(level = "debug", skip_all, fields(anchor_count = extract_anchors(spans).len()))]
pub fn calculate_layout_id(spans: &[TextSpan]) -> String {
    let anchors = extract_anchors(spans);
    let top_anchors = if anchors.len() > MAX_ANCHORS {
        &anchors[..MAX_ANCHORS]
    } else {
        &anchors[..]
    };

    if top_anchors.len() < 2 {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"fallback-no-sufficient-anchors");
        let total_len: usize = spans.iter().map(|s| s.text.len()).sum();
        hasher.update(&total_len.to_le_bytes());
        let count = spans.len() as u64;
        hasher.update(&count.to_le_bytes());
        if let Some(first) = spans.first() {
            hasher.update(&first.x0.to_le_bytes());
            hasher.update(&first.y0.to_le_bytes());
        }
        if let Some(last) = spans.last() {
            hasher.update(&last.x0.to_le_bytes());
            hasher.update(&last.y0.to_le_bytes());
        }
        return hasher.finalize().to_hex().to_string();
    }

    let distances = compute_anchor_distances(spans);
    let mut hasher = blake3::Hasher::new();
    for (dx, dy) in &distances {
        let vector_str = format!("{:.2},{:.2};", dx, dy);
        hasher.update(vector_str.as_bytes());
    }

    hasher.finalize().to_hex().to_string()
}

/// Persistent layout database backed by sled.
/// Maps layout_id → compiled WASM bytes.
pub struct LayoutDb {
    db: sled::Db,
    path: PathBuf,
}

impl LayoutDb {
    #[tracing::instrument(skip_all)]
    pub fn open(path: &Path) -> Result<Self, sled::Error> {
        let db = sled::open(path)?;
        Ok(Self {
            db,
            path: path.to_path_buf(),
        })
    }

    #[tracing::instrument(skip(self), fields(layout_id = %layout_id))]
    pub fn lookup(&self, layout_id: &str) -> Result<Option<Vec<u8>>, sled::Error> {
        self.db
            .get(layout_id.as_bytes())
            .map(|v| v.map(|iv| iv.to_vec()))
    }

    #[tracing::instrument(skip(self, wasm_bytes), fields(layout_id = %layout_id))]
    pub fn store(&self, layout_id: &str, wasm_bytes: &[u8]) -> Result<(), sled::Error> {
        self.db.insert(layout_id.as_bytes(), wasm_bytes)?;
        self.db.flush()?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn list_layouts(&self) -> Vec<String> {
        self.db
            .iter()
            .filter_map(|r| r.ok())
            .filter_map(|(k, _)| String::from_utf8(k.to_vec()).ok())
            .collect()
    }

    #[tracing::instrument(skip(self), fields(layout_id = %layout_id))]
    pub fn remove(&self, layout_id: &str) -> Result<(), sled::Error> {
        self.db.remove(layout_id.as_bytes())?;
        self.db.flush()?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[tracing::instrument(skip(self), fields(layout_id = %layout_id))]
    pub fn validate_layout(&self, layout_id: &str) -> LayoutHealth {
        let wasm_path = self.path.join(format!("{}.wasm", layout_id));
        let manifest_path = self.path.join(layout_id).join("manifest.json");
        let source_path = self.path.join(layout_id).join("source.rs");

        let wasm_ok = wasm_path.exists()
            && fs::metadata(&wasm_path)
                .map(|m| m.len() > 0)
                .unwrap_or(false);
        let manifest_ok = manifest_path.exists();
        let source_ok = source_path.exists();
        let sled_ok = self.lookup(layout_id).unwrap_or(None).is_some();

        LayoutHealth {
            layout_id: layout_id.to_string(),
            wasm_ok,
            manifest_ok,
            source_ok,
            sled_ok,
        }
    }

    #[tracing::instrument(skip(self), fields(layout_id = %layout_id))]
    pub fn repair_layout(&self, layout_id: &str) -> Result<(), String> {
        let health = self.validate_layout(layout_id);
        let wasm_path = self.path.join(format!("{}.wasm", layout_id));
        let artifact_dir = self.path.join(layout_id);
        let _ = fs::create_dir_all(&artifact_dir);

        if !health.wasm_ok && health.sled_ok {
            if let Ok(Some(bytes)) = self.lookup(layout_id) {
                let _ = fs::write(&wasm_path, &bytes);
                return Ok(());
            }
        }

        if !health.sled_ok && health.wasm_ok {
            match fs::read(&wasm_path) {
                Ok(bytes) => {
                    let _ = self.store(layout_id, &bytes);
                    return Ok(());
                }
                Err(_) => {}
            }
        }

        if !health.wasm_ok && !health.sled_ok {
            let _ = fs::remove_dir_all(&artifact_dir);
            let _ = self.remove(layout_id);
            return Err(format!("Cannot repair {}: no WASM bytes found", layout_id));
        }

        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize)]
/// Health status of a cached layout's artifacts.
pub struct LayoutHealth {
    pub layout_id: String,
    pub wasm_ok: bool,
    pub manifest_ok: bool,
    pub source_ok: bool,
    pub sled_ok: bool,
}

impl std::fmt::Debug for LayoutDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayoutDb")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

/// Checks if a pre-compiled WASM module exists for this layout ID.
/// Tries file-based lookup first (fast, no sled overhead), then sled LayoutDb.
#[tracing::instrument(level = "debug", skip_all)]
pub fn is_layout_cached(layout_id: &str, cache_dir: &Path) -> bool {
    let wasm_path = cache_dir.join(format!("{}.wasm", layout_id));
    if wasm_path.exists() {
        return true;
    }
    if let Ok(db) = LayoutDb::open(cache_dir) {
        if let Ok(Some(_)) = db.lookup(layout_id) {
            return true;
        }
    }
    false
}

/// Checks if a layout is cached using an existing LayoutDb reference (avoids reopening sled).
#[tracing::instrument(level = "debug", skip(db))]
pub fn is_layout_cached_with_db(layout_id: &str, db: &LayoutDb, cache_dir: &Path) -> bool {
    if db.lookup(layout_id).unwrap_or(None).is_some() {
        return true;
    }
    let wasm_path = cache_dir.join(format!("{}.wasm", layout_id));
    wasm_path.exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anchor_heuristics() {
        assert!(is_potential_anchor("Invoice Number:"));
        assert!(is_potential_anchor("INVOICE"));
        assert!(is_potential_anchor("Total"));
        assert!(!is_potential_anchor("Acme Corp"));
        assert!(!is_potential_anchor("INV-2026-001"));
    }

    #[test]
    fn test_anchor_detector_custom_keywords() {
        let detector = AnchorDetector::with_keywords(vec!["vendor".into(), "po".into()]);
        assert!(detect_anchor(&detector, "Vendor Name"));
        assert!(detect_anchor(&detector, "PO Number:"));
        // Colon-suffixed strings always match (built-in heuristic)
        assert!(detect_anchor(&detector, "Shipping:"));
        // Default keyword "total" still active since keywords extend defaults
        assert!(detect_anchor(&detector, "Total"));
        // Word-boundary match: "po" as standalone keyword matches "PO" in "PO Number:"
        assert!(detect_anchor(&detector, "PO"));
        // "totality" no longer matches as substring — word-boundary required for single-word keywords
        assert!(!detect_anchor(&detector, "totality"));
    }

    #[test]
    fn test_compute_anchor_distances() {
        let spans = vec![
            TextSpan {
                text: "INVOICE".to_string(),
                x0: 50.0,
                y0: 750.0,
                x1: 150.0,
                y1: 770.0,
            },
            TextSpan {
                text: "Invoice Number:".to_string(),
                x0: 50.0,
                y0: 700.0,
                x1: 150.0,
                y1: 715.0,
            },
            TextSpan {
                text: "Date:".to_string(),
                x0: 50.0,
                y0: 680.0,
                x1: 100.0,
                y1: 695.0,
            },
        ];
        let distances = compute_anchor_distances(&spans);
        assert_eq!(distances.len(), 2);
        // INVOICE (x=100) → Invoice Number: (x=100): dx ≈ 0
        let (dx0, dy0) = distances[0];
        assert!((dx0 - 0.0).abs() < 1.0);
        assert!((dy0 + 52.5).abs() < 1.0);
    }

    #[test]
    fn test_layout_id_translation_invariance() {
        let mut spans = vec![
            TextSpan {
                text: "INVOICE".to_string(),
                x0: 50.0,
                y0: 750.0,
                x1: 150.0,
                y1: 770.0,
            },
            TextSpan {
                text: "Invoice Number:".to_string(),
                x0: 50.0,
                y0: 700.0,
                x1: 150.0,
                y1: 715.0,
            },
            TextSpan {
                text: "Date:".to_string(),
                x0: 50.0,
                y0: 680.0,
                x1: 100.0,
                y1: 695.0,
            },
        ];

        let hash1 = calculate_layout_id(&spans);

        for s in &mut spans {
            s.x0 += 100.0;
            s.x1 += 100.0;
            s.y0 -= 50.0;
            s.y1 -= 50.0;
        }

        let hash2 = calculate_layout_id(&spans);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_layout_db_store_and_lookup() {
        let dir = tempfile::tempdir().unwrap();
        let db = LayoutDb::open(dir.path()).unwrap();

        let layout_id = "abc123def";
        let wasm_bytes = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]; // minimal wasm header

        assert!(db.lookup(layout_id).unwrap().is_none());

        db.store(layout_id, &wasm_bytes).unwrap();
        let retrieved = db.lookup(layout_id).unwrap();
        assert_eq!(retrieved, Some(wasm_bytes.clone()));

        let layouts = db.list_layouts();
        assert_eq!(layouts, vec![layout_id.to_string()]);

        db.remove(layout_id).unwrap();
        assert!(db.lookup(layout_id).unwrap().is_none());
    }

    #[test]
    fn test_is_layout_cached_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let layout_id = "test_layout_42";
        let wasm_path = dir.path().join(format!("{}.wasm", layout_id));
        std::fs::write(&wasm_path, b"fake wasm").unwrap();

        assert!(is_layout_cached(layout_id, dir.path()));

        std::fs::remove_file(&wasm_path).unwrap();
        assert!(!is_layout_cached(layout_id, dir.path()));
    }
}
