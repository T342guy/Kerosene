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
   art/*.png ──alchemy──► materials/*.voidtex + *.voidmat ─┐
   art/*.obj ──forge────► models/*.voidmdl ─────────────┤
                                                     ├─vault─► content.vault
   maps/*.voidmap ─cleave─► *.voidbsp ─umbra─► +vis ─radiance─► +light ─┘
        ▲                                              │
        └──────────────── chisel ◄─────────────────────┴──► void
```

---

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

**Units are inches, Z is up.** One unit is one inch; a player is 72 tall and 32
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
docs/             architecture, formats, and the tool reference
```

Read [`docs/architecture.md`](docs/architecture.md) for how the pieces fit,
[`docs/formats.md`](docs/formats.md) for the file formats, and
[`docs/tools.md`](docs/tools.md) for the full tool reference.

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
- **Chisel's 3D view is painter-sorted, not GPU-rendered.** It shows shape and
  scale accurately, but not textures or lighting, and faces that interpenetrate
  can sort wrong — brush geometry rarely does. The compiled map in the engine
  is one keystroke away.
- **No block compression for textures.** `.voidtex` is uncompressed. A bad BC
  encoder is worse than none.
- **Sound is not implemented.**

## Licence

MIT or Apache-2.0, at your option.
