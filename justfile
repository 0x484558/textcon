#!/usr/bin/env just

# Show all available recipes
default:
  @just --list

# Run tests
test:
  cargo test

# Check code formatting
fmt:
  cargo fmt -- --check

# Fix code formatting
fmt-fix:
  cargo fmt

# Run clippy with standard settings
clippy:
  cargo clippy -- -D warnings

# Run clippy with pedantic and strict settings
clippy-pedantic:
  cargo clippy -- -W clippy::pedantic -W clippy::nursery -W clippy::all -D warnings

# Build the project
build:
  cargo build

# Build for release
release:
  cargo build --release

# Run all verification steps (fmt, clippy-pedantic, test)
verify: fmt clippy-pedantic test
