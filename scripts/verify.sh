#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
sh scripts/smoke.sh
python3 flatpak/flatpak-cargo-generator.py Cargo.lock -o /tmp/gosh-midi-cargo-sources.check.json
cmp flatpak/cargo-sources.json /tmp/gosh-midi-cargo-sources.check.json
