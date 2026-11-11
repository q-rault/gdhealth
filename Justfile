set dotenv-load
set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

test: test-rust

test-rust:
    cargo test --quiet

build:
    cargo build --release --quiet
