.PHONY: build test lint fmt run analyze clean

build:
	cargo build --release

test:
	cargo test

lint:
	cargo clippy --all-targets -- -D warnings

fmt:
	cargo fmt --all

run:
	cargo run --release --bin bkash_desktop

# Decode a Dart AOT payload: make analyze LIBAPP=/path/to/libapp.so
LIBAPP ?= ../bkash-re/dartpkg/lib/arm64-v8a/libapp.so
analyze:
	./target/release/bkanalyze $(LIBAPP) --urls --features --json features.json

clean:
	cargo clean
