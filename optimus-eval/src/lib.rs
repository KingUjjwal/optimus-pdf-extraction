//! Deterministic quality evaluation for the Optimus extraction pipeline.
//!
//! Runs the offline (no-LLM) path over a scored corpus and reports field-F1
//! (schema field keys vs ground truth) and key-value precision/recall. Kept
//! dependency-light by design — only `serde_json` beyond the workspace crates.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Hand-authored expectations for one document.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct GroundTruth {
    /// Expected schema field keys (snake_case), e.g. `["invoice_number", "date"]`.
    pub fields: Vec<String>,
    /// Optional expected key-value pairs (label -> value substrings).
    #[serde(default)]
    pub kv: Vec<(String, String)>,
}

/// Evaluation result for one document.
#[derive(Debug, Clone, Serialize)]
pub struct CaseResult {
    pub name: String,
    pub spans: usize,
    pub field_precision: f64,
    pub field_recall: f64,
    pub field_f1: f64,
    pub kv_precision: f64,
    pub kv_recall: f64,
    pub schema: String,
}

/// Run the offline pipeline on one PDF and score it against ground truth.
pub fn evaluate_fixture(pdf_path: &Path, gt_path: &Path) -> Result<CaseResult> {
    let gt_str = std::fs::read_to_string(gt_path)
        .with_context(|| format!("read ground truth {:?}", gt_path))?;
    let gt: GroundTruth = serde_json::from_str(&gt_str)
        .with_context(|| format!("parse ground truth {:?}", gt_path))?;

    let spans = optimus_core::extract_spans(pdf_path)
        .with_context(|| format!("extract spans from {:?}", pdf_path))?;
    let schema = optimus_agent::infer_schema(&spans);

    let predicted: std::collections::HashSet<String> =
        serde_json::from_str::<serde_json::Value>(&schema)
            .ok()
            .and_then(|v| v.as_object().map(|o| o.keys().cloned().collect()))
            .unwrap_or_default();
    let expected: std::collections::HashSet<String> =
        gt.fields.iter().map(|f| f.to_lowercase()).collect();

    let tp = predicted.intersection(&expected).count();
    let field_precision = if predicted.is_empty() {
        0.0
    } else {
        tp as f64 / predicted.len() as f64
    };
    let field_recall = if expected.is_empty() {
        0.0
    } else {
        tp as f64 / expected.len() as f64
    };
    let field_f1 = if field_precision + field_recall > 0.0 {
        2.0 * field_precision * field_recall / (field_precision + field_recall)
    } else {
        0.0
    };

    // Key-value precision/recall over the deterministic KV detector.
    let kv = optimus_agent::detect_key_value_pairs(&spans);
    let mut kv_precision = 0.0;
    let mut kv_recall = 0.0;
    if !gt.kv.is_empty() {
        let mut hits = 0usize;
        for (label, value_sub) in &gt.kv {
            if kv.iter().any(|p| {
                p.label.to_lowercase().contains(&label.to_lowercase())
                    && p.value.contains(value_sub)
            }) {
                hits += 1;
            }
        }
        kv_recall = hits as f64 / gt.kv.len() as f64;
        kv_precision = if kv.is_empty() {
            0.0
        } else {
            hits as f64 / kv.len().max(1) as f64
        };
    }

    Ok(CaseResult {
        name: pdf_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "?".into()),
        spans: spans.len(),
        field_precision,
        field_recall,
        field_f1,
        kv_precision,
        kv_recall,
        schema,
    })
}

/// Evaluate every `*.pdf` in `fixtures_dir` that has a sibling
/// `*.ground_truth.json`.
pub fn evaluate_corpus(fixtures_dir: &Path) -> Result<Vec<CaseResult>> {
    let mut results = Vec::new();
    for entry in std::fs::read_dir(fixtures_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "pdf") {
            continue;
        }
        let stem = path.with_extension("");
        let gt_path = stem.with_extension("ground_truth.json");
        if !gt_path.exists() {
            continue;
        }
        match evaluate_fixture(&path, &gt_path) {
            Ok(r) => results.push(r),
            Err(e) => {
                eprintln!("SKIP {:?}: {}", path, e);
            }
        }
    }
    results.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gt_parses_from_json() {
        let gt: GroundTruth = serde_json::from_str(
            r#"{"fields":["invoice_number","date","total"],"kv":[["Invoice Number","INV"]]}"#,
        )
        .unwrap();
        assert_eq!(gt.fields.len(), 3);
        assert_eq!(gt.kv[0].0, "Invoice Number");
    }
}
