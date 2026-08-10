use optimus_agent::TokenUsage;
use optimus_core::{PdfTypeResult, TextQualityReport, TextSpan};
use optimus_runtime::ExtractedRecord;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bounds {
    pub min_x: f32,
    pub max_x: f32,
    pub min_y: f32,
    pub max_y: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestResult {
    pub spans: Vec<TextSpan>,
    pub count: usize,
    pub quality: TextQualityReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestFullResult {
    pub spans: Vec<TextSpan>,
    pub count: usize,
    pub grid: String,
    pub flat_graph: String,
    pub layout_id: String,
    pub is_cached: bool,
    pub bounding_box: Bounds,
    pub quality: TextQualityReport,
    pub classification: PdfTypeResult,
    pub needs_ocr: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionResult {
    pub record: ExtractedRecord,
    pub spans: Vec<TextSpan>,
    pub layout_id: String,
    pub was_cached: bool,
    pub duration_ms: u64,
    /// Per-field extraction confidence ("exact" | "heuristic" | "fuzzy"),
    /// empty when the generated WASM did not emit it.
    pub field_confidence: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaInferenceResult {
    pub schema: String,
    pub provider_used: String,
    pub token_usage: TokenUsage,
    pub fallback: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileResult {
    pub size_bytes: usize,
    pub compile_attempts: u32,
    pub extraction_attempts: u32,
    pub llm_fix_attempts: u32,
    pub token_usage: TokenUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub layout_id: String,
    pub schema: String,
    pub model: String,
    pub compile_attempts: u32,
    pub created_at: String,
    pub cache_version: u32,
}
