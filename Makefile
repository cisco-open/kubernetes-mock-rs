test:
	cargo test

clippy:
	cargo clippy -- -D clippy::all -D clippy::pedantic

doc:
	cargo doc --open
