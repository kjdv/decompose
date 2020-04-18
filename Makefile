all: build

build:
	cargo build

update:
	cargo update

test: check unit-test

check:
	cargo check --bins --examples --tests

unit-test:
	cargo test -- --test-threads=1

format:
	cargo fmt

clean:
	cargo clean

.PHONY: all build test update check unit-test clean
