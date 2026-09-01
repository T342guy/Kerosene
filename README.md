# Kerosene

A brush-based 3D game engine in Rust, built the way Valve's Source engine is
built: levels are convex solids carved into a BSP tree, visibility and lighting
are computed once at build time by separate command-line compilers, and the
whole thing is driven by a suite of standalone tools rather than a single
monolithic application.

That last part is the point. Source's real design achievement was never its
renderer — it was that Hammer, `vbsp`, `vvis`, `vrad`, `studiomdl` and VTFEdit
are *separate programs* sharing file formats. You can script them, run them on
a build server, replace one, or write your own. Kerosene keeps that shape.

```
   art/*.png ──alchemy──► materials/*.kerotex + *.keromat ─────────────┐
   art/*.obj ──forge────► models/*.keromdl ────────────────────────────┤
   sound/*.{wav,flac,mp3} ──timbre──► sound/*.keroaud ─────────────────┤
   maps/*.keromap ─cleave─► *.kerobsp ─umbra─► +vis ─radiance─► +light ┤
                                                                       └─vault─► content.vault ─► kerosene

   chisel drives all of it: edits the map, runs the compilers, launches kerosene.
   kiln   runs the same pipeline over a whole project, with no editor.
```

> **Not a Valve product.** Kerosene is an independent reimplementation. It is
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

Distances are **kerosene units** (`ku`); one is an inch. A player is 72 ku tall and
runs at 320 ku/s, so a comfortable corridor is about 128 ku and a room worth
standing in is 256 ku to the ceiling. Speeds are `ku/s`, angles are degrees,
and Z is up.

The scale is inherited from Quake and Source, and the reason to keep it is that
powers of two land on architectural sizes: a 16 ku grid gives stair risers and
door frames that are already right. See
[`kerosene_math::units`](crates/kerosene-math/src/units.rs).

## The tools

Nine programs, each with its own name, none of them the engine.

| Tool | Does | Source analogue |
|---|---|---|
| **Chisel** | The world editor. Four viewports, brush editing, entity I/O wiring, compile-and-run. | Hammer |
| **Cleave** | `.keromap` → `.kerobsp`. CSG, BSP tree, portals, leak detection. | `vbsp` |
| **Umbra** | Computes the PVS — which parts of a level can see which. | `vvis` |
| **Radiance** | Bakes static lighting into lightmaps. | `vrad` |
| **Alchemy** | Compiles textures and authors materials. | VTFEdit / `vtex` |
| **Timbre** | Compiles sounds — WAV, FLAC or MP3. Has a window, with a waveform and a gain slider. | (Source has no equivalent) |
| **Forge** | Compiles source meshes into engine models. | `studiomdl` |
| **Vault** | Packs a content tree into one archive. | `vpk` |
| **Kiln** | Runs the whole pipeline over a project. | the batch file everyone writes |

The engine itself is `kerosene`.

---

## Quick start

Requires a Rust toolchain (edition 2024; developed against 1.94).

```sh
cargo build --release              # engine and all nine tools
./scripts/build-content.sh         # compile the sample content and map
cargo run --release -p kerosene-runtime
```

**The map compile is not optional.** Art and maps are committed as sources —
`.png`, `.obj`, `.wav`, `.keromap` — and the engine loads only compiled
`.kerotex`, `.keromdl` and `.kerobsp`. Skip the script and the game will tell
you which map has never been compiled and what to run; textures it now handles
itself, because Chisel builds them on the way to opening its window and again
before every compile. On Linux the audio backend also needs ALSA headers
(`libasound2-dev`, or `alsa-lib-devel`); without them, build with
`--no-default-features` and everything but the sound works.

Nothing has to be run from the repository root. Every tool and the engine find
the content tree the same way, with the same code, and each says which answer
it took. The reliable way to settle it is a **project file** — a `.keroproj`
naming the content directory, like the one at the top of this repository:

```
project
{
    "name"     "Kerosene"
    "content"  "content"
    "startmap" "kero_start"
}
```

Without one the tree is inferred by climbing for a directory that looks like a
content root, which works and is why a fresh clone needs no setup. A project
file is how you overrule the guess, and `startmap` is why `kerosene` above needs
no `+map`.

Once the tools are built, **`kiln`** builds a project's content — textures,
models, maps, and the archive — from anywhere. It is a program rather than a
shell script because a script is not shipped: install the toolchain somewhere
and the thing that knows how to use it would stay behind in a git checkout.
`scripts/build-content.sh` is a wrapper that builds the tools from source and
regenerates the sample map, then calls it.

To open the sample level in the editor:

```sh
cargo run --release -p chisel -- content/maps/kero_start.keromap
```

Chisel builds the content tree's textures before it finishes loading, so the
editor opens with the textures in it rather than with a note about how to get
them. It skips anything already compiled, so the second start costs a
directory walk; `--no-build` turns it off. `F9` compiles and runs the map and
builds the textures again first, so one you added since opening the editor is
compiled before the map that uses it, and `view → reload textures` picks up a
build done outside without restarting.

Point entities are drawn as what they are — a lamp for a light, a figure for
the player start — and `M` opens an asset browser with names, folders, a search
and a rendered preview for every model.

A brush's type — world, `func_detail`, `func_door`, `trigger_multiple` — is a
setting at the top of its panel, with that type's settings underneath and
nothing to press first; picking a trigger textures it invisible for you.
Wiring is grouped by event, so a sequence reads as "do this, then that", and
`logic_branch` is there for the times the answer is "otherwise".

Selecting brushes also shows what they will compile as: `tools/clip` says "blocks players only", `tools/trigger` says "not
solid; touching it fires its entity's outputs". That answer comes from Cleave's
own material table, so the editor cannot disagree with the compiler. Selecting
a door draws where it opens to.

Select something and it wears eight resize grips — drag a corner to scale both
axes, an edge to scale one. The **shape** tool (`5`) draws what a box cannot:
wedges, cylinders, cones, arches and staircases, generated as however many
brushes the shape needs and undone in one step. Which pane you draw in decides
which way it stands.

`ctrl-S` saves; a map that has never been saved is asked for a name rather
than being written somewhere you would have to go looking for.
`file → rename…` moves a map and takes what was compiled from it along, so a
renamed map is not shadowed by a `.kerobsp` under its old name. The title bar
and the status bar both name the file, with a `*` when it has unsaved changes.

`` ` `` opens the developer console, `` ` `` or escape closes it. It says what
it holds the first time you open it — `find`, `help` and `cvarlist` are how you
get at the rest — and everything the engine logs appears in it as it happens.

No display? The engine runs headless — which is what a dedicated server is,
not a testing mode bolted on the side:

```sh
cargo run -p kerosene-runtime -- --headless 640 +map kero_start
```

---

## Compiling a map by hand

The three stages are separate on purpose. Each reads and writes files, so you
can stop after any of them, run them from a Makefile, or parallelise them
across a build farm.

```sh
cleave   content/maps/kero_start.keromap    # → .kerobsp and .keroprt
umbra    content/maps/kero_start.kerobsp    # → adds visibility
radiance content/maps/kero_start.kerobsp    # → adds lighting
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

**Kerosene units, Z up.** One kerosene unit is one inch; a player is 72 ku tall and 32
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
  kerosene-math       vectors, planes, convex windings with exact clipping
  kerosene-kv         KeyValues, the text format .keromap and materials use
  kerosene-config     engineconf.keroconfig — the settings every program shares
  kerosene-console    convars, concommands, the command buffer
  kerosene-vfs        layered search paths and the .vault archive format
  kerosene-asset      .kerotex textures, .keromat materials, .keromdl models
  kerosene-map        .keromap — the editable map format
  kerosene-bsp        .kerobsp — the compiled map, plus tracing and PVS
  kerosene-physics    player movement and collision response
  kerosene-entity     entities, their fields, and the I/O event queue
  kerosene-render     the wgpu renderer, lightmap atlas, PVS culling
  kerosene-engine     the host: ties it together, with and without a window
  kerosene-game       entity classes — the game DLL analogue
tools/
  chisel cleave umbra radiance alchemy forge vault kiln
apps/
  kerosene        the runtime
kerosene.keroproj  the project file: what content tree this is, and where
content/          sample art, models, materials, the sample level, and the
                  archive packed from them -- a content tree, the thing every
                  tool and the engine go looking for
docs/             architecture, formats, tools, scripting, audio,
                  licensing, configuration
```

Read [`docs/architecture.md`](docs/architecture.md) for how the pieces fit,
[`docs/formats.md`](docs/formats.md) for the file formats, and
[`docs/tools.md`](docs/tools.md) for the full tool reference.
[`docs/scripting.md`](docs/scripting.md) covers the script API,
[`docs/audio.md`](docs/audio.md) sound,
[`docs/configuration.md`](docs/configuration.md) the engine config, and
[`docs/licensing.md`](docs/licensing.md) the dependency audit and the
provenance of the algorithms.

---

## Status

Everything above works end to end: you can draw a level in Chisel, compile it
through all three stages, and walk around it. 1219 tests cover the pieces and
the seams between them, including a suite that builds a map in memory,
compiles it, loads it and plays it.

Known limits, stated plainly:

- **No networking yet.** The engine is structured for a client/server split —
  the simulation runs without a display, which is the hard part — but the
  wire protocol and prediction are not written.
- **No skeletal animation.** `.keromdl` carries bones and per-vertex weights, and
  Forge preserves them, but nothing animates them yet.
- **Chisel's 3D view is software-rasterised, not GPU-rendered.** Occlusion is
  correct — it has a real depth buffer — and it draws materials, mipped and
  perspective-correct. There is no lighting and there are no shadows, and it
  reads the *compiled* textures, so the content has to be built first. The
  compiled map in the engine is one keystroke away.
- **No block compression for textures.** `.kerotex` is uncompressed. A bad BC
  encoder is worse than none.
- **Sound is stereo, and does not know about walls.** Falloff and panning are
  there; occlusion, reverb and doppler are not, so a sound through a wall is
  as loud as one in the room.

## Licence

**LGPL-3.0-or-later OR MPL-2.0** — a dual licence; you may use Kerosene under
either one. The full texts are `LICENSE-LGPL-3.0` and `LICENSE-MPL-2.0`, and
every source file carries an `SPDX-License-Identifier: LGPL-3.0-or-later OR
MPL-2.0` line.

Pick whichever fits. MPL-2.0 is weak, *file-level* copyleft: changes *to
Kerosene's own files* must be published under MPL-2.0, but a game built on it
— your code, your assets, your levels — can be whatever you like and shipped
any way you like, with no linking stage or relinking clause. LGPL-3.0-or-later
is stronger copyleft on the engine as a whole, for anyone who wants that
guarantee. The project's preference, under either licence: if you change
Kerosene itself, contribute the change back as a pull request rather than
releasing a modified fork. [`docs/licensing.md`](docs/licensing.md) explains
all of this properly, along with the full dependency audit and the provenance
of the algorithms.
