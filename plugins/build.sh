#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
TREE_TARGET_DIR="$PROJECT_ROOT/src/tree"
ALIGNMENT_TARGET_DIR="$PROJECT_ROOT/src/alignment"

echo "Building WASM plugins..."
mkdir -p "$TREE_TARGET_DIR" "$ALIGNMENT_TARGET_DIR"

# Build newick plugin
echo "- Building the newick plugin..."
cd "$SCRIPT_DIR/newick"
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/newick.wasm "$TREE_TARGET_DIR/newick.wasm"
echo "  Copied newick.wasm to $TREE_TARGET_DIR/newick.wasm"

# Build alignment plugin
echo "- Building the alignment plugin..."
cd "$SCRIPT_DIR/alignment"
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/seq_align.wasm "$ALIGNMENT_TARGET_DIR/alignment.wasm"
echo "  Copied alignment.wasm to $ALIGNMENT_TARGET_DIR/alignment.wasm"

echo "Done! All plugins built and copied to their domain directories."
