use criterion::{black_box, criterion_group, criterion_main, Criterion};
use optimus_core::{generate_ascii_grid, generate_ascii_grid_with_config, GridConfig, GridFormat, TextSpan};

fn gen_spans(n: usize) -> Vec<TextSpan> {
    let mut spans = Vec::with_capacity(n);
    for i in 0..n {
        let x = (i % 80) as f32 * 7.0;
        let y = (i / 80) as f32 * 14.0 + 50.0;
        spans.push(TextSpan {
            text: format!("span_{:04}", i),
            x0: x,
            y0: y,
            x1: x + 50.0,
            y1: y + 10.0,
        });
    }
    spans
}

fn bench_ascii_grid(c: &mut Criterion) {
    let mut group = c.benchmark_group("ascii_grid");
    group.sample_size(50);

    for &n in &[100, 500, 1000] {
        let spans = gen_spans(n);
        group.bench_function(format!("ascii_{}_spans", n), |b| {
            b.iter(|| generate_ascii_grid(black_box(&spans)))
        });
    }

    let spans = gen_spans(500);
    group.bench_function("markdown_500_spans", |b| {
        b.iter(|| {
            generate_ascii_grid_with_config(
                black_box(&spans),
                GridConfig::default(),
                GridFormat::MarkdownTable,
            )
        })
    });

    group.finish();
}

criterion_group!(benches, bench_ascii_grid);
criterion_main!(benches);
