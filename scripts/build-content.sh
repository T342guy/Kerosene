#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#
# Compiles the sample content, from anywhere.
#
# The map compile is not optional. Maps are committed as sources -- .kmap text
# you can read and diff -- and the engine loads only compiled .kbsp. This script
# exists because the thing that knows how to run the toolset should not be a
# paragraph in a README that goes stale.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

tools="${KEROSENE_TOOLS:-}"
if [[ -z "$tools" ]]; then
    for candidate in build/release/bin build/debug/bin build/nogfx/bin; do
        if [[ -x "$candidate/kerosene-tools" ]]; then
            tools="$candidate/kerosene-tools"
            break
        fi
    done
fi

if [[ -z "$tools" ]]; then
    echo "kerosene-tools not built. Configure and build first:" >&2
    echo "    cmake --preset nogfx && cmake --build --preset nogfx" >&2
    echo "or set KEROSENE_TOOLS to its path." >&2
    exit 1
fi

echo "==> using $tools"

shopt -s nullglob
maps=(content/maps/*.kmap)
if [[ ${#maps[@]} -eq 0 ]]; then
    echo "no maps under content/maps" >&2
    exit 1
fi

for map in "${maps[@]}"; do
    echo "==> cleave $map"
    "$tools" cleave "$map"

    bsp="${map%.kmap}.kbsp"
    echo "==> umbra $bsp"
    "$tools" umbra "$bsp"
done

echo "==> content built"
