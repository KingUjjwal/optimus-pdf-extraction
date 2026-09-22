# Optimus fuzz targets

libFuzzer targets for the untrusted-input parsers (`lopdf`, `pdf_oxide`, and the
hand-rolled content-stream scanner). This crate is detached from the main
workspace and is **not** built by CI — it requires a nightly toolchain.

## Setup

```sh
rustup toolchain install nightly
cargo install cargo-fuzz
```

## Run

```sh
make fuzz                 # 60s smoke run of pdf_classify
make fuzz TARGET=pdf_extract
cargo +nightly fuzz run pdf_classify -- -max_total_time=600
```

Corpus and crash artifacts land in `fuzz/corpus/` and `fuzz/artifacts/`
(git-ignored). The `proptest` property tests in `optimus-core` cover the same
pure functions on every `cargo test` run as a fast, always-on complement.
