#!/usr/bin/env bash
# Builds the multithreaded browser wallet into frontend/pkg-threads (wasm-bindgen-rayon over
# orchard/halo2's rayon parallelism). Needs a nightly toolchain with rust-src and the wasm32 target:
#   rustup toolchain install nightly && rustup target add wasm32-unknown-unknown --toolchain nightly \
#     && rustup component add rust-src --toolchain nightly
# The linker flags matter: without --shared-memory/--import-memory the module gets a private memory
# and the thread-pool workers cannot receive it ("#<Memory> could not be cloned").
set -euo pipefail
cd "$(dirname "$0")"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-nightly}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$(pwd)/../../target/wasm-threads}"
export RUSTFLAGS='-C target-feature=+atomics,+bulk-memory,+mutable-globals -C link-arg=--shared-memory -C link-arg=--import-memory -C link-arg=--max-memory=2147483648 -C link-arg=--export=__wasm_init_tls -C link-arg=--export=__tls_size -C link-arg=--export=__tls_align -C link-arg=--export=__tls_base'
wasm-pack build . --target web --release --out-dir ../pkg-threads -- --features threads -Z build-std=panic_abort,std
