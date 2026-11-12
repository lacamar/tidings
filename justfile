default: run

run:
    cargo run --release

test:
    cargo test

lint:
    cargo clippy --all-targets
    cargo fmt --check
