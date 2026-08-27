#!/usr/bin/env bash
# Build every piece of shipped content from its sources.
#
# Compiled content is not committed -- it is reproducible from the .vmap,
# .png and .obj files that are. Run this after cloning, or after changing
# anything under content/art or content/maps.
set -euo pipefail
cd "$(dirname "$0")/.."

PROFILE="${PROFILE:-debug}"
BIN="target/$PROFILE"
CARGO_FLAGS=""
[ "$PROFILE" = "release" ] && CARGO_FLAGS="--release"

echo "==> building tools"
cargo build --quiet $CARGO_FLAGS -p cleave -p umbra -p radiance -p alchemy -p forge -p vault

echo "==> alchemy: textures and materials"
"$BIN/alchemy" batch content/art -o content/materials --make-materials
# The sky needs the sky shader rather than the lit default.
"$BIN/alchemy" material dev/sky_void --shader sky --basetexture dev/sky_void \
    -o content/materials/dev/sky_void.vmat

echo "==> forge: models"
"$BIN/forge" compile content/art/props/crate.obj -o content/models/props/crate.vmdl --scale-metres

echo "==> map compile"
cargo run --quiet $CARGO_FLAGS -p void-map --example sample_map
for map in content/maps/*.vmap; do
    name="${map%.vmap}"
    echo "--- $(basename "$map")"
    "$BIN/cleave"   "$map"
    "$BIN/umbra"    "$name.vbsp"
    "$BIN/radiance" "$name.vbsp"
done

echo "==> vault: packing"
"$BIN/vault" pack content -o void_content.vault \
    --ext vtex --ext vmat --ext vmdl --ext vbsp
"$BIN/vault" verify void_content.vault

echo
echo "content is built. Run the engine with:"
echo "    cargo run $CARGO_FLAGS -p void-runtime -- +map void_start"
