#!/usr/bin/env bash
# Build every piece of shipped content from its sources.
#
# Compiled content -- .voidtex, .voidbsp, .voidmdl, .vault -- is not committed.
# It is reproducible from the .voidmap, .png, .obj, .voidmat and .wav files
# that are. Run this after cloning, or after changing anything under
# content/art or content/maps.
#
# Chisel runs the texture half of this itself, on the way to opening its
# window, so an editor session does not depend on anyone having run this. The
# map compile is the part that still has to happen here or from the editor's
# F9: nothing runs a level compile behind your back.
set -euo pipefail
cd "$(dirname "$0")/.."

PROFILE="${PROFILE:-debug}"
BIN="target/$PROFILE"
CARGO_FLAGS=""
[ "$PROFILE" = "release" ] && CARGO_FLAGS="--release"

echo "==> building tools"
cargo build --quiet $CARGO_FLAGS -p cleave -p umbra -p radiance -p alchemy -p forge -p vault

echo "==> alchemy: textures and materials"
# One call, because Chisel makes the same one. Two callers with two ideas of
# what "build the textures" meant is how the editor came to open with no
# textures in it while this script insisted everything was fine.
"$BIN/alchemy" build content

echo "==> forge: models"
"$BIN/forge" compile content/art/props/crate.obj -o content/models/props/crate.voidmdl --scale-metres

echo "==> map compile"
cargo run --quiet $CARGO_FLAGS -p void-map --example sample_map
for map in content/maps/*.voidmap; do
    name="${map%.voidmap}"
    echo "--- $(basename "$map")"
    "$BIN/cleave"   "$map"
    "$BIN/umbra"    "$name.voidbsp"
    "$BIN/radiance" "$name.voidbsp"
done

echo "==> vault: packing"
# Into the content tree, not beside it: that is where a shipped game keeps its
# archives, and it is where the engine looks without being told. The pack walk
# ignores .vault itself, so writing the archive into the tree it packs is not
# the problem it looks like.
"$BIN/vault" pack content -o content/void_content.vault \
    --ext voidtex --ext voidmat --ext voidmdl --ext voidbsp \
    --ext voidscript --ext voidsnd --ext wav
"$BIN/vault" verify content/void_content.vault

echo
echo "content is built. Run the engine with:"
echo "    cargo run $CARGO_FLAGS -p void-runtime -- +map void_start"
