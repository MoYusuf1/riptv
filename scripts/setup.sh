#!/usr/bin/env bash
set -euo pipefail

build_only=false
server_args=()
for arg in "$@"; do
    case "$arg" in
        --build-only) build_only=true ;;
        --logs) server_args+=(--logs) ;;
        --help|-h)
            echo "Usage: ./scripts/setup.sh [--build-only] [--logs]"
            echo "Installs missing Rust build tools, builds RIPTV, and starts it locally."
            echo "Requires Rust, Git, FFmpeg and a system C compiler (see docs/setup.md)."
            exit 0 ;;
        *) echo "Unknown option: $arg. Use --help for options." >&2; exit 2 ;;
    esac
done

cd "$(dirname "${BASH_SOURCE[0]}")/.."

for tool in cargo rustc git ffmpeg ffprobe cc; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "Missing $tool. Follow docs/setup.md, then run this script again." >&2
        exit 1
    fi
done

wasm_libdir="$(rustc --print target-libdir --target wasm32-unknown-unknown)"
if ! compgen -G "$wasm_libdir/libcore-*.rlib" >/dev/null; then
    if command -v rustup >/dev/null 2>&1; then
        echo "Installing the Rust WebAssembly target..."
        rustup target add wasm32-unknown-unknown
    else
        echo "Install your distribution's wasm32-unknown-unknown Rust target, then retry." >&2
        exit 1
    fi
fi

if ! command -v dx >/dev/null 2>&1 || [[ ! "$(dx --version)" =~ ^dioxus[[:space:]]0\.7\.10([[:space:]]|$) ]]; then
    echo "Installing Dioxus CLI 0.7.10 (the first installation can take several minutes)..."
    cargo install dioxus-cli --version 0.7.10 --locked
    if ! command -v dx >/dev/null 2>&1; then
        echo "Dioxus was installed, but dx is not on PATH. Reopen your terminal and retry." >&2
        exit 1
    fi
fi

echo "Building RIPTV..."
dx build --web --release --locked -p app
cargo build --release --locked -p riptv

if $build_only; then
    echo "Build complete. Run 'cargo riptv' to start RIPTV."
    exit 0
fi

echo "Open http://127.0.0.1:${IPTV_PORT:-3000} once RIPTV starts. Press Ctrl+C to stop it."
exec cargo run --release --locked -p riptv -- "${server_args[@]}"
