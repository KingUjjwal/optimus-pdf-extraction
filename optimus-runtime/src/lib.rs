use anyhow::{anyhow, Result};
use arrow::array::{ArrayRef, StringBuilder};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use wasmtime::*;

/// Generic record resulting from PDF extraction. Scalar fields are JSON strings;
/// array fields (e.g. `transactions`) are JSON arrays of objects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedRecord {
    #[serde(flatten)]
    pub fields: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailureStage {
    Ingestion,
    Routing,
    Compilation,
    Extraction,
}

impl std::fmt::Display for FailureStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FailureStage::Ingestion => write!(f, "Ingestion"),
            FailureStage::Routing => write!(f, "Routing"),
            FailureStage::Compilation => write!(f, "Compilation"),
            FailureStage::Extraction => write!(f, "Extraction"),
        }
    }
}

/// A record of a failed PDF processing attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessFailure {
    pub path: PathBuf,
    pub stage: FailureStage,
    pub error: String,
}

/// Aggregate summary of batch PDF processing results.
#[derive(Debug, Clone)]
pub struct ProcessSummary {
    pub success: usize,
    pub failed: Vec<ProcessFailure>,
    pub records: Vec<ExtractedRecord>,
}

/// Per-file batch result: source path plus either an extracted record or a
/// failure tagged with the pipeline stage it occurred in.
type ProcessResult = (PathBuf, Result<ExtractedRecord, (FailureStage, String)>);

impl ProcessSummary {
    #[tracing::instrument(skip(self))]
    pub fn total(&self) -> usize {
        self.success + self.failed.len()
    }
}

/// Wasmtime-based WASM execution host with module pre-compilation cache.
pub struct WasmHost {
    engine: Engine,
    module_cache: Arc<parking_lot::RwLock<HashMap<String, Module>>>,
}

impl Default for WasmHost {
    fn default() -> Self {
        Self::new()
    }
}

impl WasmHost {
    /// Initializes and configures the embedded Wasmtime dynamic engine with module pre-compilation cache.
    #[tracing::instrument]
    pub fn new() -> Self {
        let mut config = Config::new();
        config.cranelift_opt_level(OptLevel::Speed);
        config.static_memory_maximum_size(100 * 1024 * 1024); // 100MB limit
        config.consume_fuel(true);
        let engine = Engine::new(&config).unwrap();
        Self {
            engine,
            module_cache: Arc::new(parking_lot::RwLock::new(HashMap::new())),
        }
    }

    /// Executes zero-copy extraction logic with module pre-compilation caching.
    #[tracing::instrument(skip(self, wasm_bytes, flat_graph), fields(layout_id = %layout_id))]
    pub fn execute_extraction(
        &self,
        layout_id: &str,
        wasm_bytes: &[u8],
        flat_graph: &str,
    ) -> Result<String> {
        let module = {
            let cache = self.module_cache.read();
            cache.get(layout_id).cloned()
        };
        if let Some(module) = module {
            return Self::run_module(&self.engine, &module, flat_graph);
        }

        let module = Module::new(&self.engine, wasm_bytes)?;
        let result = Self::run_module(&self.engine, &module, flat_graph)?;
        {
            let mut cache = self.module_cache.write();
            cache
                .entry(layout_id.to_string())
                .or_insert_with(|| module.clone());
        }
        Ok(result)
    }

    /// Pre-compile a WASM module and store it in the cache without executing.
    #[tracing::instrument(skip(self, wasm_bytes), fields(layout_id = %layout_id))]
    pub fn precompile(&self, layout_id: &str, wasm_bytes: &[u8]) -> Result<()> {
        let module = Module::new(&self.engine, wasm_bytes)?;
        let mut cache = self.module_cache.write();
        cache.insert(layout_id.to_string(), module);
        Ok(())
    }

    /// Check if a layout is already cached.
    #[tracing::instrument(skip(self), fields(layout_id = %layout_id))]
    pub fn is_cached(&self, layout_id: &str) -> bool {
        self.module_cache.read().contains_key(layout_id)
    }

    /// Core WASM execution against a pre-compiled Module.
    fn run_module(engine: &Engine, module: &Module, flat_graph: &str) -> Result<String> {
        let mut store = Store::new(engine, ());
        // Set fuel to limit WASM execution to ~10M instructions
        store.set_fuel(1_000_000_000u64)?;
        let linker = Linker::new(engine);
        let instance = linker.instantiate(&mut store, module)?;

        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| anyhow!("Failed to export memory from WASM guest"))?;

        let alloc_fn = instance.get_typed_func::<u32, u32>(&mut store, "alloc")?;
        let extract_fn = instance.get_typed_func::<(u32, u32), u32>(&mut store, "extract")?;
        let free_buf_fn = instance.get_typed_func::<(u32, u32), ()>(&mut store, "free_buf")?;

        let graph_bytes = flat_graph.as_bytes();
        let graph_len = graph_bytes.len() as u32;

        let guest_ptr = alloc_fn.call(&mut store, graph_len)?;
        memory.write(&mut store, guest_ptr as usize, graph_bytes)?;

        let result_ptr = extract_fn.call(&mut store, (guest_ptr, graph_len))?;

        if result_ptr == 0 {
            let _ = free_buf_fn.call(&mut store, (guest_ptr, graph_len));
            return Err(anyhow!("WASM extract returned a null pointer"));
        }

        let mut len_buf = [0u8; 4];
        memory.read(&store, result_ptr as usize, &mut len_buf)?;
        let result_len = u32::from_le_bytes(len_buf);

        if result_len > 10 * 1024 * 1024 {
            let _ = free_buf_fn.call(&mut store, (guest_ptr, graph_len));
            return Err(anyhow!(
                "WASM result length {} exceeds 10MB limit",
                result_len
            ));
        }

        let mut result_bytes = vec![0u8; result_len as usize];
        memory.read(&store, (result_ptr + 4) as usize, &mut result_bytes)?;

        let result_str = String::from_utf8(result_bytes)?;

        free_buf_fn.call(&mut store, (result_ptr, result_len + 4))?;
        free_buf_fn.call(&mut store, (guest_ptr, graph_len))?;

        Ok(result_str)
    }
}

/// Shared extraction pipeline: spans → graph → layout_id → cache/compile → WASM execute → record.
/// Consolidates the 5 duplicate extraction pipelines across CLI, runtime, and Tauri.
#[tracing::instrument(skip(host, spans, cache_dir), fields(span_count = spans.len()))]
pub fn extract_from_spans(
    host: &WasmHost,
    spans: &[optimus_core::TextSpan],
    cache_dir: &Path,
) -> Result<(ExtractedRecord, String, bool)> {
    let graph = optimus_core::build_spatial_graph(spans.to_vec());
    let flat_graph = optimus_agent::serialize_flat_graph(&graph);
    let core_spans: Vec<_> = graph.nodes.iter().map(|n| n.span.clone()).collect();
    let layout_id = optimus_router::calculate_layout_id(&core_spans);

    let is_cached = optimus_router::is_layout_cached(&layout_id, cache_dir)
        && optimus_agent::manifest_cache_current(&layout_id, cache_dir);

    let wasm_bytes = if is_cached {
        let wasm_path = cache_dir.join(format!("{}.wasm", layout_id));
        std::fs::read(&wasm_path)
            .map_err(|e| anyhow!("read cached wasm for {}: {}", layout_id, e))?
    } else {
        let schema = optimus_agent::discover_schema_from_spans(&core_spans);
        optimus_agent::compile_extraction_logic(&layout_id, &graph, &schema, cache_dir)
            .map_err(|e| anyhow!("compile {}: {}", layout_id, e))?
    };

    let output_json = host.execute_extraction(&layout_id, &wasm_bytes, &flat_graph)?;
    let record: ExtractedRecord = serde_json::from_str(&output_json)?;

    Ok((record, layout_id, is_cached))
}

/// Dynamic high-concurrency PDF ingestion and JIT extraction pipeline powered by Rayon.
#[tracing::instrument(level = "info", skip(paths, cache_dir), fields(count = paths.len()))]
pub fn process_pdfs_parallel(paths: &[&Path], cache_dir: &Path) -> Vec<Result<ExtractedRecord>> {
    let summary = process_pdfs_summary(paths, cache_dir);
    let mut results: Vec<Result<ExtractedRecord>> = summary.records.into_iter().map(Ok).collect();
    for failure in summary.failed {
        results.push(Err(anyhow!(
            "[{}] {}",
            failure.path.display(),
            failure.error
        )));
    }
    results
}

/// Streaming pipeline that accepts an Iterator of paths, processes them in parallel using par_bridge,
/// and sends Arrow RecordBatches to a channel in chunks.
#[tracing::instrument(level = "info", skip(paths_iter, cache_dir))]
pub fn process_pdfs_streaming<I>(
    paths_iter: I,
    cache_dir: &Path,
    chunk_size: usize,
) -> std::sync::mpsc::Receiver<Result<RecordBatch>>
where
    I: Iterator<Item = PathBuf> + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    let cache_dir_owned = cache_dir.to_path_buf();

    std::thread::spawn(move || {
        let host = Arc::new(WasmHost::new());
        let cache_dir_arc = Arc::new(cache_dir_owned);

        let (record_tx, record_rx) = std::sync::mpsc::channel();

        // Spawn the parallel extraction
        let cache_ref = Arc::clone(&cache_dir_arc);
        let host_ref = Arc::clone(&host);

        rayon::spawn(move || {
            paths_iter
                .par_bridge()
                .for_each_with(record_tx, |tx, path| {
                    let path_ref = path.as_path();
                    let spans = match optimus_core::extract_spans(path_ref) {
                        Ok(s) => s,
                        Err(_) => return,
                    };

                    if let Ok((record, _, _)) = extract_from_spans(&host_ref, &spans, &cache_ref) {
                        let _ = tx.send(record);
                    }
                });
        });

        // Batch up results and emit Arrow RecordBatches
        let mut buffer = Vec::with_capacity(chunk_size);
        while let Ok(record) = record_rx.recv() {
            buffer.push(record);
            if buffer.len() >= chunk_size {
                if let Ok(batch) = build_arrow_record_batch(&buffer) {
                    if tx.send(Ok(batch)).is_err() {
                        return; // Receiver dropped
                    }
                }
                buffer.clear();
            }
        }

        // Send final chunk
        if !buffer.is_empty() {
            if let Ok(batch) = build_arrow_record_batch(&buffer) {
                let _ = tx.send(Ok(batch));
            }
        }
    });

    rx
}

/// Processes PDFs with error aggregation into ProcessSummary.
/// Shares a single WasmHost across all Rayon threads for optimal module cache reuse.
#[tracing::instrument(skip(paths, cache_dir), fields(count = paths.len()))]
pub fn process_pdfs_summary(paths: &[&Path], cache_dir: &Path) -> ProcessSummary {
    let host = Arc::new(WasmHost::new());
    let cache_dir_owned = cache_dir.to_path_buf();

    let results: Vec<ProcessResult> = paths
        .par_iter()
        .map(|path| {
            let path_buf = path.to_path_buf();

            let spans = match optimus_core::extract_spans(path) {
                Ok(s) => s,
                Err(e) => {
                    return (
                        path_buf,
                        Err((FailureStage::Ingestion, format!("extract_spans: {}", e))),
                    )
                }
            };

            match extract_from_spans(&host, &spans, &cache_dir_owned) {
                Ok((record, _, _)) => (path_buf, Ok(record)),
                Err(e) => (path_buf, Err((FailureStage::Extraction, format!("{}", e)))),
            }
        })
        .collect();

    let mut success = 0usize;
    let mut failed = Vec::new();
    let mut records = Vec::new();

    for (path, result) in results {
        match result {
            Ok(record) => {
                success += 1;
                records.push(record);
            }
            Err((stage, error)) => {
                failed.push(ProcessFailure { path, stage, error });
            }
        }
    }

    ProcessSummary {
        success,
        failed,
        records,
    }
}

/// Serializes aggregated extraction records into analytics-ready Apache Arrow record batches.
#[tracing::instrument(skip(records))]
pub fn build_arrow_record_batch(records: &[ExtractedRecord]) -> Result<RecordBatch> {
    if records.is_empty() {
        return build_dynamic_arrow_batch(&[], &[]);
    }
    let field_names: Vec<&str> = records[0].fields.keys().map(|s| s.as_str()).collect();
    let records_json: Vec<serde_json::Value> = records
        .iter()
        .map(|r| {
            serde_json::Value::Object(
                r.fields
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            )
        })
        .collect();
    build_dynamic_arrow_batch(&records_json, &field_names)
}

/// Builds an Arrow RecordBatch from a flexible schema and dynamic JSON values.
/// Each field in `field_names` becomes a Utf8 column.
#[tracing::instrument(level = "debug", skip_all)]
pub fn build_dynamic_arrow_batch(
    records: &[serde_json::Value],
    field_names: &[&str],
) -> Result<RecordBatch> {
    if records.is_empty() {
        let fields: Vec<Field> = field_names
            .iter()
            .map(|name| Field::new(*name, DataType::Utf8, true))
            .collect();
        let schema = Arc::new(Schema::new(fields));
        return Ok(RecordBatch::new_empty(schema));
    }

    let mut builders: Vec<StringBuilder> = (0..field_names.len())
        .map(|_| StringBuilder::new())
        .collect();

    for record in records {
        for (i, name) in field_names.iter().enumerate() {
            match record.get(*name) {
                Some(serde_json::Value::Null) | None => {
                    builders[i].append_null();
                }
                Some(v) => {
                    let val = match v {
                        serde_json::Value::String(s) => s.clone(),
                        serde_json::Value::Number(n) => n.to_string(),
                        serde_json::Value::Bool(b) => b.to_string(),
                        other => other.to_string(),
                    };
                    builders[i].append_value(&val);
                }
            }
        }
    }

    let arrays: Vec<ArrayRef> = builders
        .into_iter()
        .map(|mut b| Arc::new(b.finish()) as ArrayRef)
        .collect();

    let fields: Vec<Field> = field_names
        .iter()
        .map(|name| Field::new(*name, DataType::Utf8, true))
        .collect();

    let schema = Arc::new(Schema::new(fields));
    RecordBatch::try_new(schema, arrays).map_err(|e| anyhow!("Arrow batch build: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use optimus_core::TextSpan;

    #[test]
    fn test_end_to_end_runtime_jit() {
        let temp_dir = std::env::temp_dir();
        let cache_dir = temp_dir.join("optimus_test_cache");
        let _ = std::fs::remove_dir_all(&cache_dir);

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
                text: "INV-2026-001".to_string(),
                x0: 180.0,
                y0: 700.0,
                x1: 280.0,
                y1: 715.0,
            },
            TextSpan {
                text: "Date:".to_string(),
                x0: 50.0,
                y0: 680.0,
                x1: 100.0,
                y1: 695.0,
            },
            TextSpan {
                text: "2026-05-23".to_string(),
                x0: 180.0,
                y0: 680.0,
                x1: 270.0,
                y1: 695.0,
            },
            TextSpan {
                text: "Total:".to_string(),
                x0: 400.0,
                y0: 400.0,
                x1: 450.0,
                y1: 415.0,
            },
            TextSpan {
                text: "$500.50".to_string(),
                x0: 500.0,
                y0: 400.0,
                x1: 555.0,
                y1: 415.0,
            },
        ];

        let graph = optimus_core::build_spatial_graph(spans);
        let flat_graph = optimus_agent::serialize_flat_graph(&graph);
        let layout_id = optimus_router::calculate_layout_id(
            &graph
                .nodes
                .iter()
                .map(|n| n.span.clone())
                .collect::<Vec<_>>(),
        );

        let wasm_bytes =
            optimus_agent::compile_extraction_logic(&layout_id, &graph, "{}", &cache_dir).unwrap();

        let host = WasmHost::new();
        let extracted_json = host
            .execute_extraction(&layout_id, &wasm_bytes, &flat_graph)
            .unwrap();

        let record: ExtractedRecord = serde_json::from_str(&extracted_json).unwrap();
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

        // Verify module cache hit: second extraction should use cached module
        let extracted_json2 = host
            .execute_extraction(&layout_id, &wasm_bytes, &flat_graph)
            .unwrap();
        assert_eq!(extracted_json, extracted_json2);
        assert!(host.is_cached(&layout_id));

        // Pre-compile test
        let host2 = WasmHost::new();
        host2.precompile(&layout_id, &wasm_bytes).unwrap();
        assert!(host2.is_cached(&layout_id));

        let batch = build_arrow_record_batch(&[record]).unwrap();
        assert_eq!(batch.num_rows(), 1);
        assert_eq!(batch.num_columns(), 3);

        let _ = std::fs::remove_dir_all(&cache_dir);
    }

    #[test]
    fn test_dynamic_arrow_batch() {
        let records = vec![
            serde_json::json!({"field_a": "hello", "field_b": "42"}),
            serde_json::json!({"field_a": "world", "field_b": "99"}),
        ];
        let batch = build_dynamic_arrow_batch(&records, &["field_a", "field_b"]).unwrap();
        assert_eq!(batch.num_rows(), 2);
        assert_eq!(batch.num_columns(), 2);
    }

    #[test]
    fn test_dynamic_arrow_empty() {
        let batch = build_dynamic_arrow_batch(&[], &["col1", "col2"]).unwrap();
        assert_eq!(batch.num_rows(), 0);
        assert_eq!(batch.num_columns(), 2);
    }
}
