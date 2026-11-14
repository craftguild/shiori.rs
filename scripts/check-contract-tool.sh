#!/bin/sh
# The canonical replay adapter is an independent build-only Rust workspace.
set -eu
cargo fmt --manifest-path tools/contract/Cargo.toml --check
cargo clippy --offline --locked --manifest-path tools/contract/Cargo.toml -- -D warnings
