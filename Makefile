all: build

build:
	cargo build

install:
	cargo install --path .

update:
	cargo update

test:
	cargo test -- --test-threads=1

check:
	cargo check --bins --examples --tests

format:
	cargo fmt

clean:
	cargo clean

.PHONY: all build test update check unit-test clean
