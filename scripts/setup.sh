#!/usr/bin/env bash
set -euo pipefail

if [[ $# -gt 1 ]] || [[ $# -eq 1 && "$1" != "--build-only" ]]; then
    echo "Usage: ./scripts/setup.sh [--build-only]" >&2
    exit 2
fi

cd "$(dirname "${BASH_SOURCE[0]}")/.."

for tool in cargo rustc git dx; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        case "$tool" in
            dx) echo "Missing Dioxus CLI. Install it with: cargo install dioxus-cli --version 0.7.10 --locked" >&2 ;;
            *) echo "Missing $tool. Install Rust (cargo/rustc) and Git, then retry." >&2 ;;
        esac
        exit 1
    fi
done

wasm_libdir="$(rustc --print target-libdir --target wasm32-unknown-unknown)"
if [[ ! -d "$wasm_libdir" ]]; then
    echo "Missing the wasm32-unknown-unknown Rust target." >&2
    echo "With rustup: rustup target add wasm32-unknown-unknown" >&2
    echo "Otherwise, install the target using your distribution's Rust packages." >&2
    exit 1
fi

if ! command -v ffmpeg >/dev/null 2>&1; then
    echo "Note: ffmpeg is not installed. Some 4K, HEVC, and unsupported streams may not play." >&2
fi

echo "Building RIPTV..."
dx build --web --release -p app

if [[ "${1:-}" == "--build-only" ]]; then
    echo "Build complete. Run 'cargo riptv' to start RIPTV."
    exit 0
fi

echo "Open http://127.0.0.1:${IPTV_PORT:-3000} once RIPTV starts. Press Ctrl+C to stop it."
cargo riptv
