use criterion::{black_box, criterion_group, criterion_main, Criterion};
use optimus_core::{build_spatial_graph, TextSpan};

fn gen_spans(n: usize) -> Vec<TextSpan> {
    let mut spans = Vec::with_capacity(n);
    for i in 0..n {
        let x = (i % 50) as f32 * 12.0;
        let y = (i / 50) as f32 * 18.0 + 50.0;
        spans.push(TextSpan {
            text: format!("span_{}", i),
            x0: x,
            y0: y,
            x1: x + 60.0,
            y1: y + 12.0,
        });
    }
    spans
}

fn bench_build_spatial_graph(c: &mut Criterion) {
    let mut group = c.benchmark_group("spatial_graph");
    group.sample_size(20);

    for &n in &[100, 500, 1000, 5000] {
        let spans = gen_spans(n);
        group.bench_function(format!("build_{}_spans", n), |b| {
            b.iter(|| build_spatial_graph(black_box(spans.clone())))
        });
    }
    group.finish();
}

criterion_group!(benches, bench_build_spatial_graph);
criterion_main!(benches);
