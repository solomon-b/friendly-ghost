default:
    @just --list

# Run all checks (fmt, clippy, tests)
check: fmt-check lint test

# Run tests
test:
    cargo test

# Run clippy
lint:
    cargo clippy -- -D warnings

# Format code
fmt:
    cargo fmt

# Check formatting without modifying
fmt-check:
    cargo fmt --check

# Build in release mode
build:
    cargo build --release

# Run with a config file
run config:
    cargo run -- --config {{config}}

# Dry run with a config file
dry-run config:
    cargo run -- --config {{config}} --dry-run
