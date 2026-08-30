#!/usr/bin/env bash
# Build this repository's content.
#
# Two things happen here that Kiln does not do, because neither belongs in a
# shipped tool: the tools get built from source, and the sample map gets
# regenerated from the code that defines it. Everything after that is
# `kiln`, which is a program rather than a script precisely so that it
# installs beside the compilers and works from a copy of the toolchain with
# no repository behind it.
#
# So: this file is a convenience for working *in* the repository. Anyone with
# the tools has `kiln`, and that is the supported way to build a project.
set -euo pipefail
cd "$(dirname "$0")/.."

PROFILE="${PROFILE:-debug}"
BIN="target/$PROFILE"
CARGO_FLAGS=""
[ "$PROFILE" = "release" ] && CARGO_FLAGS="--release"

echo "==> building tools"
cargo build --quiet $CARGO_FLAGS \
    -p kiln -p cleave -p umbra -p radiance -p alchemy -p forge -p vault

echo "==> regenerating the sample map from its source"
# The sample level is defined in code so that a change to the map format shows
# up as a compile error rather than as a level that silently stops loading.
cargo run --quiet $CARGO_FLAGS -p kerosene-map --example sample_map

"$BIN/kiln" "$@"

echo
echo "Run the engine with:"
echo "    cargo run $CARGO_FLAGS -p kerosene-runtime"
