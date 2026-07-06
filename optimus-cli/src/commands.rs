use anyhow::Result;
use std::path::Path;

#[tracing::instrument(level = "info", skip_all, fields(path = %path.display()))]
pub fn extract_single(path: &Path, cache: &Path, format: &str) -> Result<()> {
    let spans = optimus_core::extract_spans(path)?;
    let host = optimus_runtime::WasmHost::new();
    let (record, layout_id, _was_cached) =
        optimus_runtime::extract_from_spans(&host, &spans, cache)?;
    eprintln!("Layout ID: {}", optimus_agent::display_id(&layout_id));

    match format {
        "arrow" => {
            let field_names: Vec<&str> = record.fields.keys().map(|k| k.as_str()).collect();
            let record_value = serde_json::to_value(&record.fields)?;
            let batch = optimus_runtime::build_dynamic_arrow_batch(&[record_value], &field_names)?;

            let mut writer =
                arrow::ipc::writer::FileWriter::try_new(std::io::stdout(), &batch.schema())?;
            writer.write(&batch)?;
            writer.finish()?;
        }
        _ => println!("{}", serde_json::to_string(&record)?),
    }

    Ok(())
}

#[tracing::instrument(level = "info", skip_all, fields(input = %input.display(), output = %output.display()))]
pub fn batch_extract(input: &Path, output: &Path, cache: &Path) -> Result<()> {
    use std::fs;
    let mut pdf_paths = Vec::new();
    for entry in fs::read_dir(input)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map_or(false, |e| e == "pdf") {
            pdf_paths.push(path);
        }
    }

    eprintln!("Found {} PDFs in {}", pdf_paths.len(), input.display());
    let path_refs: Vec<&Path> = pdf_paths.iter().map(|p| p.as_path()).collect();
    let summary = optimus_runtime::process_pdfs_summary(&path_refs, cache);

    eprintln!(
        "Done: {} success, {} failed",
        summary.success,
        summary.failed.len()
    );
    for f in &summary.failed {
        eprintln!("  FAIL [{}]: {} — {}", f.stage, f.path.display(), f.error);
    }

    let records_json: Vec<serde_json::Value> = summary
        .records
        .iter()
        .map(|r| {
            serde_json::Value::Object(
                r.fields
                    .iter()
                    .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                    .collect(),
            )
        })
        .collect();

    let field_names: Vec<&str> = if records_json.is_empty() {
        vec![]
    } else {
        records_json[0]
            .as_object()
            .map(|o| o.keys().map(|k| k.as_str()).collect())
            .unwrap_or_default()
    };

    let batch = optimus_runtime::build_dynamic_arrow_batch(&records_json, &field_names)?;

    let out_file = fs::File::create(output)?;
    let mut writer = arrow::ipc::writer::FileWriter::try_new(out_file, &batch.schema())?;
    writer.write(&batch)?;
    writer.finish()?;

    eprintln!("Arrow IPC written to {}", output.display());
    Ok(())
}

pub fn cache_list(cache: &Path) -> Result<()> {
    if let Ok(db) = optimus_router::LayoutDb::open(cache) {
        for id in db.list_layouts() {
            println!("{}", id);
        }
    } else {
        println!("No cache found at {}", cache.display());
    }
    Ok(())
}

pub fn cache_clear(cache: &Path) -> Result<()> {
    if let Ok(db) = optimus_router::LayoutDb::open(cache) {
        for id in db.list_layouts() {
            db.remove(&id)?;
        }
    }
    let _ = std::fs::remove_dir_all(cache);
    println!("Cache cleared: {}", cache.display());
    Ok(())
}

pub fn generate_grid(path: &Path, format: &str) -> Result<()> {
    let spans = optimus_core::extract_spans(path)?;
    let grid_format = match format.to_lowercase().as_str() {
        "markdown" => optimus_core::GridFormat::MarkdownTable,
        _ => optimus_core::GridFormat::Ascii,
    };

    let grid_str = optimus_core::generate_ascii_grid_with_config(
        &spans,
        optimus_core::GridConfig::default(),
        grid_format,
    );

    println!("{}", grid_str);
    Ok(())
}

#[tracing::instrument(level = "info", skip_all)]
pub fn ingest(input: &Path, cache: &Path) -> Result<()> {
    use std::fs;
    let mut pdf_paths = Vec::new();
    for entry in fs::read_dir(input)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map_or(false, |e| e == "pdf") {
            pdf_paths.push(path);
        }
    }

    eprintln!(
        "Ingesting {} PDFs from {}",
        pdf_paths.len(),
        input.display()
    );
    let db = optimus_router::LayoutDb::open(cache)?;

    let mut new_layouts = 0;
    for path in pdf_paths {
        if let Ok(spans) = optimus_core::extract_spans(&path) {
            let layout_id = optimus_router::calculate_layout_id(&spans);
            if !optimus_router::is_layout_cached_with_db(&layout_id, &db, cache) {
                let graph = optimus_core::build_spatial_graph(spans);
                let core_spans: Vec<_> = graph.nodes.iter().map(|n| n.span.clone()).collect();
                let schema = optimus_agent::infer_schema(&core_spans);
                if optimus_agent::compile_extraction_logic(&layout_id, &graph, &schema, cache)
                    .is_ok()
                {
                    let _ = db.store(&layout_id, b"cached");
                    new_layouts += 1;
                }
            }
        }
    }

    eprintln!("Ingestion complete. {} new layouts cached.", new_layouts);
    Ok(())
}

pub fn status(cache: &Path) -> Result<()> {
    let db = optimus_router::LayoutDb::open(cache)?;
    let layouts = db.list_layouts();
    println!("Cache directory: {}", cache.display());
    println!("Total cached layouts: {}", layouts.len());

    for id in layouts {
        let wasm_path = cache.join(format!("{}.wasm", id));
        let size = std::fs::metadata(&wasm_path).map(|m| m.len()).unwrap_or(0);
        println!("  - {}: {} bytes", optimus_agent::display_id(&id), size);
    }
    Ok(())
}

#[tracing::instrument(level = "info", skip_all, fields(count))]
pub fn benchmark(count: usize, cache: &Path) -> Result<()> {
    use std::time::Instant;

    let db = optimus_router::LayoutDb::open(cache)?;
    let layouts = db.list_layouts();
    if layouts.is_empty() {
        return Err(anyhow::anyhow!("No layouts in cache. Run ingest first."));
    }

    let host = optimus_runtime::WasmHost::new();
    let mut wasm_modules = std::collections::HashMap::new();
    let flat_graph = "mock|graph|data\n".to_string(); // Mock flat graph

    for id in &layouts {
        let wasm_path = cache.join(format!("{}.wasm", id));
        if let Ok(bytes) = std::fs::read(&wasm_path) {
            let _ = host.precompile(id, &bytes);
            wasm_modules.insert(id.clone(), bytes);
        }
    }

    eprintln!(
        "Benchmarking {} iterations across {} layouts...",
        count,
        layouts.len()
    );
    let mut latencies = Vec::with_capacity(count);
    let start = Instant::now();

    for i in 0..count {
        let idx = i % layouts.len();
        let layout_id = &layouts[idx];
        let bytes = &wasm_modules[layout_id];

        let iter_start = Instant::now();
        let _ = host.execute_extraction(layout_id, bytes, &flat_graph);
        latencies.push(iter_start.elapsed().as_micros());
    }

    let total_time = start.elapsed();
    latencies.sort_unstable();

    let p50 = latencies[count / 2];
    let p95 = latencies[(count as f64 * 0.95) as usize];
    let p99 = latencies[(count as f64 * 0.99) as usize];
    let avg = latencies.iter().copied().sum::<u128>() as f64 / count as f64;
    let docs_per_sec = count as f64 / total_time.as_secs_f64();

    println!("--- Benchmark Results ---");
    println!("Total time: {:.2?}", total_time);
    println!("Throughput: {:.2} docs/sec", docs_per_sec);
    println!("Avg Latency: {:.2} µs", avg);
    println!("p50 Latency: {} µs", p50);
    println!("p95 Latency: {} µs", p95);
    println!("p99 Latency: {} µs", p99);

    Ok(())
}

use notify::event::AccessKind;
use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::sync::mpsc::channel;

#[tracing::instrument(level = "info", skip_all, fields(dir = %dir.display()))]
pub fn watch(dir: &Path, cache: &Path) -> Result<()> {
    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        eprintln!("\nShutting down watcher...");
        r.store(false, std::sync::atomic::Ordering::SeqCst);
    })?;

    println!("Watching directory for new PDFs: {}", dir.display());
    println!("Press Ctrl+C to stop.");
    let (tx, rx) = channel();

    let mut watcher = notify::recommended_watcher(tx)?;
    watcher.watch(dir, RecursiveMode::Recursive)?;

    while running.load(std::sync::atomic::Ordering::SeqCst) {
        match rx.recv_timeout(std::time::Duration::from_millis(500)) {
            Ok(res) => match res {
                Ok(Event {
                    kind: EventKind::Access(AccessKind::Close(_)),
                    paths,
                    ..
                }) => {
                    for path in paths {
                        if path.extension().map_or(false, |e| e == "pdf") {
                            println!("New PDF detected: {}", path.display());
                            match extract_single(&path, cache, "json") {
                                Ok(_) => println!("Successfully extracted {}", path.display()),
                                Err(e) => eprintln!("Failed to extract {}: {}", path.display(), e),
                            }
                        }
                    }
                }
                Ok(Event {
                    kind: EventKind::Create(_),
                    paths,
                    ..
                }) => {
                    for path in paths {
                        if path.extension().map_or(false, |e| e == "pdf") {
                            std::thread::sleep(std::time::Duration::from_millis(500));
                            println!("New PDF detected: {}", path.display());
                            match extract_single(&path, cache, "json") {
                                Ok(_) => println!("Successfully extracted {}", path.display()),
                                Err(e) => eprintln!("Failed to extract {}: {}", path.display(), e),
                            }
                        }
                    }
                }
                _ => {
                    tracing::trace!("Unhandled notify event: {:?}", res);
                }
            },
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    println!("Watcher stopped.");
    Ok(())
}
