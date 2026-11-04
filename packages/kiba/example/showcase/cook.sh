#!/bin/sh
# Cooks every generated model into assets/showcase, and writes their bounds for showcase.rs.
set -e
cd "$(dirname "$0")/.."
mkdir -p assets/showcase
for model in showcase/models/*.glb; do
    name=$(basename "$model" .glb)
    cargo run -q -p kiba --features cook --bin kiba-cook -- \
        model "assets/showcase/$name.kiba" --max-texture 512 "$model"
done
{ echo '['; python3 -I showcase/bounds.py showcase/models/*.glb; echo ']'; } > showcase/bounds.rs
