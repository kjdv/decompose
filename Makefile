all: build

build:
	cargo build

update:
	cargo update

test: check test-unit

check:
	cargo check --bins --examples --tests

test-unit:
	cargo test -- --test-threads=1

format:
	cargo fmt

clean:
	cargo clean

.PHONY: all build test update check unit-test clean
