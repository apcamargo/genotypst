#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
TARGET_DIR="$PROJECT_ROOT/src"

echo "Building WASM plugins..."

# Build newick plugin
echo "- Building the newick plugin..."
cd "$SCRIPT_DIR/newick"
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/newick.wasm "$TARGET_DIR/"
echo "  Copied newick.wasm to src/"

# Build alignment plugin
echo "- Building the alignment plugin..."
cd "$SCRIPT_DIR/alignment"
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/seq_align.wasm "$TARGET_DIR/alignment.wasm"
echo "  Copied alignment.wasm to src/"

echo "Done! All plugins built and copied to $TARGET_DIR"
