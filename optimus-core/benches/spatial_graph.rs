use criterion::{black_box, criterion_group, criterion_main, Criterion};
use optimus_core::{
    build_spatial_graph, generate_ascii_grid_with_config, GridConfig, GridFormat, TextSpan,
};
use rand::Rng;

fn random_spans(n: usize) -> Vec<TextSpan> {
    let mut rng = rand::thread_rng();
    let labels = [
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
                    labels[i].to_string()
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

fn bench_build_spatial_graph(c: &mut Criterion) {
    let mut group = c.benchmark_group("spatial_graph");
    group.sample_size(20);

    for &n in &[100, 1000] {
        let spans = random_spans(n);
        group.bench_function(format!("build_{}_spans", n), |b| {
            b.iter(|| build_spatial_graph(black_box(spans.clone())))
        });
    }
    group.finish();
}

fn bench_ascii_grid_default(c: &mut Criterion) {
    let spans = random_spans(1000);
    c.bench_function("ascii_grid_default_1000_spans", |b| {
        b.iter(|| {
            generate_ascii_grid_with_config(
                black_box(&spans),
                GridConfig::default(),
                GridFormat::Ascii,
            )
        })
    });
}

criterion_group!(benches, bench_build_spatial_graph, bench_ascii_grid_default,);
criterion_main!(benches);
