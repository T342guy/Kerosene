# Kerosene

A brush-based 3D game engine in C++, built the way Valve's Source engine is
built: levels are convex solids carved into a BSP tree, visibility and lighting
are computed once at build time by compilers, and the engine loads the result.
The compilers and the editor are one executable, `kerosene-tools`, kept apart
from the engine by the same boundary Source kept between its tools and its game
DLL.

That boundary is the point. Source's real design achievement was never its
renderer — it was that Hammer, `vbsp`, `vvis`, `vrad` and `studiomdl` are
*separate from the game* and share file formats. You can script them, run them
on a build server, replace one, or write your own. Kerosene keeps that shape,
and here the boundary is a build rule rather than a convention: an engine
library that reaches into the toolset fails configuration.

```
   maps/*.kmap ─cleave─► *.kbsp ─umbra─► +vis ─radiance─► +light ─► kerosene
```

> **Not a Valve product.** Kerosene is an independent reimplementation. It is
> not affiliated with, endorsed by, or sponsored by Valve Corporation or id
> Software, and contains none of their source code, assets or data files. It
> cannot open Source or Quake content and does not try to — every format it
> defines is its own, deliberately named and byte-tagged so it cannot be
> mistaken for anyone else's. "Valve", "Source", "Hammer" and "Quake" are their
> owners' trademarks, and appear here only to say what a piece of this project
> is analogous to. See [`NOTICE`](NOTICE).

---

## Units

Distances are **kerosene units** (`ku`); one is two inches, about 5 cm. A player
is 36 ku tall and runs at 160 ku/s, so a comfortable corridor is 64 ku and a
room worth standing in is 128 ku to the ceiling. Angles are pitch/yaw/roll with
pitch positive *downward*, and Z is up.

The scale is Quake's, halved. What that scale was ever for is that powers of two
land on architectural sizes — a 4 ku grid gives stair risers and door frames
that are already right — and two inches keeps that exactly while making every
number half as long. It also buys precision: the world edge is at 8192 rather
than 16384, which is one more bit of float mantissa in the corners where CSG
produces slivers.

See [`src/math/units.hpp`](src/math/units.hpp).

## The tools

One executable, `kerosene-tools`, with a subcommand per stage. Each reads and
writes files, so you can stop after any of them, run them from a Makefile, or
parallelise them across a build farm.

| Tool | Does | Source analogue |
|---|---|---|
| **Cleave** | `.kmap` → `.kbsp`. CSG, the BSP tree, portals, leak detection. | `vbsp` |
| **Umbra** | The PVS — which parts of a level can see which. | `vvis` |
| Radiance | Bakes static lighting into lightmaps. | `vrad` |
| Alchemy | Compiles textures and authors materials. | VTFEdit / `vtex` |
| Forge | Compiles source meshes into engine models. | `studiomdl` |
| Vault | Packs a content tree into one archive. | `vpk` |
| Chisel | The world editor. | Hammer |
| Kiln | Runs the whole pipeline over a project. | the batch file everyone writes |

Cleave and Umbra are implemented. The rest are named here because the shape of
the toolset is a design decision, not a wish list — see
[Status](#status) for what actually exists.

---

## Quick start

Kerosene is developed on an image-based Fedora desktop, where the host carries
no development headers. `scripts/devshell.sh` creates a container with them and
leaves the host image alone:

```sh
./scripts/devshell.sh                 # create and enter the dev container
cmake --preset debug
cmake --build --preset debug
ctest --preset debug
```

On a traditional distribution, install the equivalent `-devel` packages and skip
the script. Everything but the renderer builds with no graphics headers at all:

```sh
cmake --preset nogfx && cmake --build --preset nogfx && ctest --preset nogfx
```

That preset is the compilers, the simulation and the whole test suite — which is
what a CI machine with no GPU runs.

### Compiling and running the sample level

```sh
./scripts/build-content.sh            # cleave + umbra over content/maps
./build/debug/bin/kerosene +map kero_start
```

**The map compile is not optional.** Maps are committed as sources — `.kmap`
text you can read and diff — and the engine loads only compiled `.kbsp`. Skip
the script and the engine will tell you which map has never been compiled and
what to run.

No display? The engine runs headless, which is what a dedicated server is rather
than a testing mode bolted on the side:

```sh
./build/nogfx/bin/kerosene --headless 400 +map kero_start
```

### Compiling a map by hand

The stages are separate on purpose.

```sh
kerosene-tools cleave content/maps/kero_start.kmap   # -> .kbsp and .kprt
kerosene-tools umbra  content/maps/kero_start.kbsp   # -> adds visibility
```

An unvised map still loads and plays; it just draws everything. That is
deliberate — you should be able to walk a level thirty seconds after drawing it.
`umbra --fast` skips the expensive pass while a layout is still moving.

---

## What "Source-like" means here

These are the properties that actually shape the engine, not surface
resemblance.

**Levels are brushes, not meshes.** A solid is the intersection of its faces'
half-spaces, stored as planes rather than vertices. Convexity is then structural
rather than something to validate, and it is what makes CSG possible.

**Everything expensive happens at build time.** Visibility, lighting, mipmaps,
surface reflectivity. The engine loads what the tools produced; it does not
compute it.

**Entity I/O instead of scripting.** A button's `OnPressed` fires a door's
`Open` after a delay. No scripting language, and it composes much further than
it has any right to — because the graph is *data*, which can be drawn, diffed
and validated without running it.

**Everything is a convar or a concommand.** Console text, key binds, `.kcfg`
files and command-line `+arguments` all take one path.

**The movement model is reproduced, not approximated.** Including the air-speed
cap that makes bunny-hopping and surfing work. That is not a bug to be fixed:
removing it would change the game.

## Where it does better

Stated plainly, so the claim is checkable.

1. **Double-precision compile geometry with one named epsilon policy.** `vbsp`
   spreads its tolerances across the files that use them, several spelled `0.1`
   inline; here each is named for what it means and chosen per scalar type,
   tight for the compilers and loose for the runtime.
2. **A job system under every compile stage**, not just the innermost loops.
3. **Leaks that name the entity and the path out**, written to `.kleak` as a
   polyline. Source tells you a leak exists and leaves you to find it.
4. **Arbitrary-size box collision.** Quake and Source precompute hulls at a few
   fixed sizes and snap every entity to the nearest; Cleave adds bevel planes so
   a sweep is exact for any box.
5. **One explicit GPU backend** — SDL3's GPU API over Vulkan/D3D12/Metal —
   replacing Source's per-API `shaderapi` DLLs.
6. **Headless-first simulation**, which is simultaneously the dedicated server,
   the CI harness and the precondition for client prediction.
7. **A tools/engine boundary the build system enforces** rather than trusts.
8. **Shaders compiled at build time**, so no shader toolchain ships or is needed
   at runtime.

---

## Layout

```
src/
  core/       types, assert, log, arena, the job system
  math/       vectors, planes, convex windings, the unit scale, the epsilons
  kv/         KeyValues, the text format maps and materials use
  console/    convars, concommands, the command buffer
  map/        .kmap -- the editable map format
  bsp/        .kbsp -- the compiled map, plus tracing and PVS decoding
  physics/    player movement and collision response
  entity/     entities, their fields, and the I/O event queue
  game/       entity classes -- the game-DLL analogue
  engine/     the host: ties it together, with and without a window
  render/     the SDL_GPU renderer
tools/
  cleave/     CSG, BSP, portals, leak detection
  umbra/      the potentially visible set
apps/
  kerosene    the runtime
content/      the sample level and its materials
docs/         architecture, formats, building
kerosene.kproj   the project file: what content tree this is, and where
```

[`docs/architecture.md`](docs/architecture.md) is how the pieces fit,
[`docs/formats.md`](docs/formats.md) the file formats, and
[`docs/building.md`](docs/building.md) the build in more detail.

---

## Status

The pipeline works end to end: a `.kmap` compiles through CSG, BSP and
visibility, and the engine loads it and lets you walk around with correct
collision. 135 tests cover the pieces and the seams, including a scripted
playthrough that walks from one room to the other and asserts the same inputs
give the same result twice.

Known limits, stated plainly:

- **No lighting.** Radiance is not written; the `.kbsp` lighting lump is
  reserved and empty, and the renderer applies a flat placeholder shade so
  surfaces at different angles can be told apart.
- **No texture compiler.** Alchemy is not written, so materials render as
  procedural developer textures — a tinted grid per material. `.kmat` files are
  read for their material names only.
- **No models, no audio, no editor.** Forge, Timbre and Chisel are named in the
  toolset table and not implemented.
- **No networking.** The engine is structured for a client/server split — the
  simulation runs without a display, which is the hard part — but the wire
  protocol and prediction are not written.
- **Brush entities do not move.** `func_door` and friends have no runtime; the
  entity I/O graph that would drive them does work.

## Licence

**LGPL-3.0-or-later OR MPL-2.0** — a dual licence; use Kerosene under either.
Every source file carries an SPDX line. MPL-2.0 is weak, file-level copyleft:
change a Kerosene file and that file stays MPL, while your game code, levels and
assets are yours. LGPL-3.0-or-later is stronger copyleft on the engine as a
whole, for anyone who wants that guarantee. [`NOTICE`](NOTICE) explains both
properly.
