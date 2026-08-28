# VoidEngine

A brush-based 3D game engine in Rust, built the way Valve's Source engine is
built: levels are convex solids carved into a BSP tree, visibility and lighting
are computed once at build time by separate command-line compilers, and the
whole thing is driven by a suite of standalone tools rather than a single
monolithic application.

That last part is the point. Source's real design achievement was never its
renderer — it was that Hammer, `vbsp`, `vvis`, `vrad`, `studiomdl` and VTFEdit
are *separate programs* sharing file formats. You can script them, run them on
a build server, replace one, or write your own. VoidEngine keeps that shape.

```
   art/*.png ──alchemy──► materials/*.voidtex + *.voidmat ──────────────┐
   art/*.obj ──forge────► models/*.voidmdl ─────────────────────────────┤
   maps/*.voidmap ─cleave─► *.voidbsp ─umbra─► +vis ─radiance─► +light ─┤
                                                                        └─vault─► content.vault ─► void

   chisel drives all of it: edits the map, runs the compilers, launches void.
```

> **Not a Valve product.** VoidEngine is an independent reimplementation. It is
> not affiliated with, endorsed by, or sponsored by Valve Corporation or id
> Software, and it contains none of their source code, assets or data files. It
> cannot open Source or Quake content and does not try to — every format it
> defines is its own, deliberately named and byte-tagged so it cannot be
> mistaken for anyone else's. Valve and id names appear throughout these docs
> for one reason only: to say what a piece of this project is analogous to.
> "Valve", "Source", "Hammer" and "Quake" are their owners' trademarks. See
> [`NOTICE`](NOTICE).

---

## Units

Distances are **void units** (`vu`); one is an inch. A player is 72 vu tall and
runs at 320 vu/s, so a comfortable corridor is about 128 vu and a room worth
standing in is 256 vu to the ceiling. Speeds are `vu/s`, angles are degrees,
and Z is up.

The scale is inherited from Quake and Source, and the reason to keep it is that
powers of two land on architectural sizes: a 16 vu grid gives stair risers and
door frames that are already right. See
[`void_math::units`](crates/void-math/src/units.rs).

## The tools

Seven programs, each with its own name, none of them the engine.

| Tool | Does | Source analogue |
|---|---|---|
| **Chisel** | The world editor. Four viewports, brush editing, entity I/O wiring, compile-and-run. | Hammer |
| **Cleave** | `.voidmap` → `.voidbsp`. CSG, BSP tree, portals, leak detection. | `vbsp` |
| **Umbra** | Computes the PVS — which parts of a level can see which. | `vvis` |
| **Radiance** | Bakes static lighting into lightmaps. | `vrad` |
| **Alchemy** | Compiles textures and authors materials. | VTFEdit / `vtex` |
| **Forge** | Compiles source meshes into engine models. | `studiomdl` |
| **Vault** | Packs a content tree into one archive. | `vpk` |

The engine itself is `void`.

---

## Quick start

Requires a Rust toolchain (edition 2024; developed against 1.94).

```sh
cargo build --release              # engine and all seven tools
./scripts/build-content.sh         # compile the sample content and map
cargo run --release -p void-runtime -- +map void_start
```

To open the sample level in the editor:

```sh
cargo run --release -p chisel -- content/maps/void_start.voidmap
```

`F9` compiles and runs it.

No display? The engine runs headless — which is what a dedicated server is,
not a testing mode bolted on the side:

```sh
cargo run -p void-runtime -- --headless 640 +map void_start
```

---

## Compiling a map by hand

The three stages are separate on purpose. Each reads and writes files, so you
can stop after any of them, run them from a Makefile, or parallelise them
across a build farm.

```sh
cleave   content/maps/void_start.voidmap    # → .voidbsp and .voidprt
umbra    content/maps/void_start.voidbsp    # → adds visibility
radiance content/maps/void_start.voidbsp    # → adds lighting
```

An unvised, unlit map still loads and plays; it just draws everything and looks
flat. That is deliberate — you should be able to walk a level thirty seconds
after drawing it.

`umbra --fast` and `radiance --fast` skip the expensive passes while a layout
is still moving.

---

## What "Source-like" means here

These are the properties that actually shape the engine, not surface
resemblance:

**Void units, Z up.** One void unit is one inch; a player is 72 vu tall and 32
wide. Angles are pitch/yaw/roll with pitch positive *downward*, a Quake
inheritance Source never corrected and neither does this.

**Levels are brushes, not meshes.** A solid is the intersection of its faces'
half-spaces, stored as planes rather than vertices. That makes convexity
structural rather than something to validate, and it is what makes CSG
possible.

**Everything expensive happens at build time.** Visibility, lighting, mipmaps,
surface reflectivity, model welding. The engine loads what the tools produced;
it does not compute it.

**Entity I/O instead of scripting.** A button's `OnPressed` fires a door's
`Open` after a delay. No scripting language, and it composes much further than
it has any right to.

**Everything is a convar or a concommand.** Console text, key binds, `.cfg`
files and command-line `+arguments` all take one path.

**The movement model is reproduced, not approximated.** Including the air-speed
cap that makes bunny-hopping and surfing work. That is not a bug to be fixed:
removing it would change the game.

---

## Layout

```
crates/
  void-math       vectors, planes, convex windings with exact clipping
  void-kv         KeyValues, the text format .voidmap and materials use
  void-console    convars, concommands, the command buffer
  void-vfs        layered search paths and the .vault archive format
  void-asset      .voidtex textures, .voidmat materials, .voidmdl models
  void-map        .voidmap — the editable map format
  void-bsp        .voidbsp — the compiled map, plus tracing and PVS
  void-physics    player movement and collision response
  void-entity     entities, their fields, and the I/O event queue
  void-render     the wgpu renderer, lightmap atlas, PVS culling
  void-engine     the host: ties it together, with and without a window
  void-game       entity classes — the game DLL analogue
tools/
  chisel cleave umbra radiance alchemy forge vault
apps/
  void            the runtime
content/          sample art, models, materials and the sample level
docs/             architecture, formats, tools, scripting, audio, licensing
```

Read [`docs/architecture.md`](docs/architecture.md) for how the pieces fit,
[`docs/formats.md`](docs/formats.md) for the file formats, and
[`docs/tools.md`](docs/tools.md) for the full tool reference.
[`docs/scripting.md`](docs/scripting.md) covers the script API,
[`docs/audio.md`](docs/audio.md) sound, and
[`docs/licensing.md`](docs/licensing.md) the dependency audit and the
provenance of the algorithms.

---

## Status

Everything above works end to end: you can draw a level in Chisel, compile it
through all three stages, and walk around it. 605 tests cover the pieces and
the seams between them, including a suite that builds a map in memory,
compiles it, loads it and plays it.

Known limits, stated plainly:

- **No networking yet.** The engine is structured for a client/server split —
  the simulation runs without a display, which is the hard part — but the
  wire protocol and prediction are not written.
- **No skeletal animation.** `.voidmdl` carries bones and per-vertex weights, and
  Forge preserves them, but nothing animates them yet.
- **Chisel's 3D view is software-rasterised, not GPU-rendered.** Occlusion is
  correct — it has a real depth buffer — but there are no textures, no
  lighting, and no shadows. It shows shape, scale and what is in front of what.
  The compiled map in the engine is one keystroke away.
- **No block compression for textures.** `.voidtex` is uncompressed. A bad BC
  encoder is worse than none.
- **Sound is stereo, and does not know about walls.** Falloff and panning are
  there; occlusion, reverb and doppler are not, so a sound through a wall is
  as loud as one in the room.

## Licence

**LGPL-3.0-or-later.** The full texts are `COPYING` (GPL-3.0, which the LGPL
builds on) and `COPYING.LESSER` (LGPL-3.0); every source file carries an
`SPDX-License-Identifier` line.

In practice: changes *to VoidEngine itself* must be published under the same
licence, but a game built on it can be whatever you like. That is the whole
point of choosing the Lesser GPL over the GPL.

One thing to know before you ship a binary: the LGPL's mechanism assumes a
user can swap in their own build of the library, and Rust links statically. If
you distribute a closed-source game linked against VoidEngine, LGPL §4 asks you
to make relinking possible — ship object files, or link the engine as a shared
library. [`docs/licensing.md`](docs/licensing.md) explains this properly, along
with the full dependency audit and the provenance of the algorithms.
