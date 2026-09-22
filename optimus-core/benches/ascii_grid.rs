use criterion::{black_box, criterion_group, criterion_main, Criterion};
use optimus_core::{generate_ascii_grid_with_config, GridConfig, GridFormat, TextSpan};
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
                font_size: 0.0,
                is_bold: false,
                is_italic: false,
            }
        })
        .collect()
}

fn bench_ascii_grid_default(c: &mut Criterion) {
    let spans = random_spans(1000);
    c.bench_function("ascii_grid_default", |b| {
        b.iter(|| {
            generate_ascii_grid_with_config(
                black_box(&spans),
                GridConfig::default(),
                GridFormat::Ascii,
            )
        })
    });
}

fn bench_ascii_grid_custom(c: &mut Criterion) {
    let spans = random_spans(1000);
    c.bench_function("ascii_grid_x4_y8", |b| {
        b.iter(|| {
            generate_ascii_grid_with_config(
                black_box(&spans),
                GridConfig {
                    x_bucket: 4,
                    y_bucket: 8,
                    include_font_size: false,
                },
                GridFormat::Ascii,
            )
        })
    });
}

criterion_group!(benches, bench_ascii_grid_default, bench_ascii_grid_custom);
criterion_main!(benches);
