use criterion::{black_box, criterion_group, criterion_main, Criterion};
use optimus_core::TextSpan;
use optimus_router::{calculate_layout_id, compute_anchor_distances, extract_anchors};
use rand::Rng;

fn random_spans(n: usize) -> Vec<TextSpan> {
    let mut rng = rand::thread_rng();
    let anchor_labels = [
        "INVOICE",
        "Invoice Number:",
        "Date:",
        "Total:",
        "Bill To:",
        "Description",
        "Quantity",
        "Unit Price",
        "Amount",
    ];
    (0..n)
        .map(|i| {
            let x0 = rng.gen_range(0.0..500.0);
            let y0 = rng.gen_range(0.0..700.0);
            TextSpan {
                text: if i < 9 {
                    anchor_labels[i].to_string()
                } else {
                    format!("item-{}", i)
                },
                x0,
                y0,
                x1: x0 + rng.gen_range(30.0..100.0),
                y1: y0 + rng.gen_range(10.0..20.0),
                page: None,
            }
        })
        .collect()
}

fn bench_extract_anchors(c: &mut Criterion) {
    let spans = random_spans(1000);
    c.bench_function("extract_anchors_1000_spans", |b| {
        b.iter(|| extract_anchors(black_box(&spans)))
    });
}

fn bench_compute_anchor_distances(c: &mut Criterion) {
    let spans = random_spans(1000);
    c.bench_function("compute_anchor_distances_1000_spans", |b| {
        b.iter(|| compute_anchor_distances(black_box(&spans)))
    });
}

fn bench_calculate_layout_id(c: &mut Criterion) {
    let spans = random_spans(1000);
    c.bench_function("calculate_layout_id_1000_spans", |b| {
        b.iter(|| calculate_layout_id(black_box(&spans)))
    });
}

criterion_group!(
    benches,
    bench_extract_anchors,
    bench_compute_anchor_distances,
    bench_calculate_layout_id,
);
criterion_main!(benches);
