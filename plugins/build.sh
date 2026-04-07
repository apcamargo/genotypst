#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
TREE_TARGET_DIR="$PROJECT_ROOT/src/tree"
ALIGNMENT_TARGET_DIR="$PROJECT_ROOT/src/alignment"
GENOME_MAP_TARGET_DIR="$PROJECT_ROOT/src/genome_map"

echo "Building WASM plugins..."
mkdir -p "$TREE_TARGET_DIR" "$ALIGNMENT_TARGET_DIR" "$GENOME_MAP_TARGET_DIR"

# Build tree plugin
echo "- Building the tree plugin..."
cd "$SCRIPT_DIR/tree"
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/tree.wasm "$TREE_TARGET_DIR/tree.wasm"
echo "  Copied tree.wasm to $TREE_TARGET_DIR/tree.wasm"

# Build alignment plugin
echo "- Building the alignment plugin..."
cd "$SCRIPT_DIR/alignment"
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/seq_align.wasm "$ALIGNMENT_TARGET_DIR/alignment.wasm"
echo "  Copied alignment.wasm to $ALIGNMENT_TARGET_DIR/alignment.wasm"

# Build genome-map plugin
echo "- Building the genome-map plugin..."
cd "$SCRIPT_DIR/genome_map"
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/genome_map.wasm "$GENOME_MAP_TARGET_DIR/genome_map.wasm"
echo "  Copied genome_map.wasm to $GENOME_MAP_TARGET_DIR/genome_map.wasm"

echo "Done! All plugins built and copied to their domain directories."
