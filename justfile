# Full pipeline: format, build plugins, compile docs
all: fmt build-plugins compile-docs

# Run all formatters
fmt: fmt-typst fmt-rust fmt-toml

# Build all WASM plugins
build-plugins: setup-wasm build-tree build-alignment build-genome-map

# Compile all documentation
compile-docs: compile-pdf compile-svgs

# Format all Typst files
fmt-typst:
    fd -e typ -x typstyle -v -i

# Format all Rust code
fmt-rust:
    fd 'Cargo.toml$' -x cargo fmt -v --manifest-path {}

# Format all TOML files
fmt-toml:
    fd -e toml -x tombi format {}

# Install WASM target
setup-wasm:
    rustup target list --installed | grep -q wasm32-unknown-unknown || rustup target add wasm32-unknown-unknown

# Build tree plugin
build-tree: setup-wasm
    cargo build --release --target wasm32-unknown-unknown --manifest-path plugins/tree/Cargo.toml
    cp plugins/tree/target/wasm32-unknown-unknown/release/tree.wasm src/tree/tree.wasm

# Build alignment plugin
build-alignment: setup-wasm
    cargo build --release --target wasm32-unknown-unknown --manifest-path plugins/alignment/Cargo.toml
    cp plugins/alignment/target/wasm32-unknown-unknown/release/seq_align.wasm src/alignment/alignment.wasm

# Build genome-map plugin
build-genome-map: setup-wasm
    cargo build --release --target wasm32-unknown-unknown --manifest-path plugins/genome_map/Cargo.toml
    cp plugins/genome_map/target/wasm32-unknown-unknown/release/genome_map.wasm src/genome_map/genome_map.wasm

# Compile the manual PDF
compile-pdf: fmt build-plugins
    typst compile --root . docs/manual.typ docs/manual.pdf

# Compile all example SVGs
compile-svgs: fmt build-plugins
    fd '_example\.typ$' docs -x typst compile --root . {} docs/svgs/{/.}_light.svg --format svg
    fd '_example\.typ$' docs -x typst compile --root . {} docs/svgs/{/.}_dark.svg --format svg --input theme=dark
