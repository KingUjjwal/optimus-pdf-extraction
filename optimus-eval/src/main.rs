use anyhow::Result;
use optimus_eval::evaluate_corpus;
use std::path::PathBuf;

fn main() -> Result<()> {
    let fixtures = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("optimus-core/tests/fixtures"));

    let results = evaluate_corpus(&fixtures)?;
    if results.is_empty() {
        eprintln!(
            "No scored fixtures found in {:?} (need `<name>.pdf` + `<name>.ground_truth.json`)",
            fixtures
        );
        return Ok(());
    }

    println!(
        "{:<20} {:>6} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "case", "spans", "f-prec", "f-recall", "f-F1", "kv-prec", "kv-recall"
    );
    println!("{}", "-".repeat(66));
    let mut f1_sum = 0.0;
    for r in &results {
        println!(
            "{:<20} {:>6} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>8.3}",
            r.name,
            r.spans,
            r.field_precision,
            r.field_recall,
            r.field_f1,
            r.kv_precision,
            r.kv_recall
        );
        f1_sum += r.field_f1;
    }
    println!("{}", "-".repeat(66));
    if !results.is_empty() {
        println!(
            "{:<20} {:>6} {:>8} {:>8} {:>8.3}",
            "mean",
            "",
            "",
            "",
            f1_sum / results.len() as f64
        );
    }
    Ok(())
}
