# Kerosene

![Kerosene](./.github/Images/kerosene-readme-banner.png)

**A Rust game crate for brush-built 3D games.** Add `kerosene` to a Cargo
project, implement one trait, and you have a game: a movement solver in the
Quake-to-Source lineage, levels compiled from convex brushes into a BSP tree
with baked visibility, lighting and acoustics, Source-style entity I/O,
physics props, a game UI, saved games and Steam. The editor and every
compiler come with it, as a library your game re-hosts, so the whole thing
builds from source on your own machine — Linux, Windows or macOS — with
nothing to install but Rust.

```sh
cargo install kerosene-tools
kerosene-tools new mygame
cd mygame
cargo play
```

(Until the first crates.io release, install from the repository:
`cargo install --git https://github.com/t342guy/kerosene kerosene-tools`.)

That is a game: a Cargo package with a `Game` of its own, a starter map, and
the editor and compilers knowing its classes. `cargo play` builds whatever
content changed and runs it; `cargo tools chisel` opens the editor;
`cargo ship` builds a copy to hand out. [Getting
started](src/gamedev/getting-started.md) walks through it.

```rust
use kerosene::prelude::*;

#[derive(Default)]
struct MyGame;

impl Game for MyGame {
    fn classes(&self, registry: &mut ClassRegistry) {
        kerosene::game::register(registry); // doors, triggers, lights, sounds
        registry.register(ClassDef::new("item_pickup"));
    }
}

fn main() -> anyhow::Result<()> {
    launch(MyGame, LaunchOptions::new("My Game", env!("CARGO_PKG_VERSION")))
}
```

It is built the way Valve's Source engine is built: levels are convex solids
carved into a BSP tree, visibility and lighting are computed once at build
time by compilers, and the engine loads the result. The compilers are kept
apart from the engine by the same boundary Source kept between its tools and
its game DLL, and that boundary is the point. Source's real design
achievement was never its renderer — it was that Hammer, `vbsp`, `vvis`,
`vrad`, `studiomdl` and VTFEdit are *separate from the game* and share file
formats. You can script them, run them on a build server, replace one, or
write your own. Kerosene keeps that shape: each stage is still a separate,
scriptable subcommand, and one binary carries them all.

```
   art/*.png ──alchemy──► materials/*.kerotex + *.keromat ─────────────┐
   textures/<name>/ ─alchemy─► a whole set: colour, normals, roughness,┤
                               emissive, occlusion + the material      │
   art/*.obj ──forge────► models/*.keromdl ────────────────────────────┤
   sound/*.{wav,flac,mp3} ──timbre──► sound/*.keroaud ─────────────────┤
   maps/*.keromap ─cleave─► *.kerobsp ─umbra─► +vis ─resonance─► +sound ─radiance─► +light ┤
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

## The tools come with it

One application, `kerosene-tools` — and, in a game made with `new`, the same
application as the game's own `mygame-tools`, knowing its classes. Open it with no arguments and you get one
window holding every tool: a project page, the world editor, the sound
editor, a build form and an archive form, switched with an activity bar down
the left edge, and one output panel every job logs into. None of it is the
engine.

| Tool | Does | Source analogue |
|---|---|---|
| **Chisel** (editor) | The world editor. Four viewports, brush editing, entity I/O wiring, compile-and-run. | Hammer |
| **Cleave** | `.keromap` → `.kerobsp`. CSG, BSP tree, portals, leak detection. | `vbsp` |
| **Umbra** | Computes the PVS — which parts of a level can see which. | `vvis` |
| **Resonance** | Works out what each room sounds like from its shape and materials, for the engine's reverb. | (Source has no equivalent) |
| **Radiance** | Bakes static lighting into lightmaps. | `vrad` |
| **Alchemy** | Compiles textures and authors materials. | VTFEdit / `vtex` |
| **Timbre** (sound) | Compiles sounds — WAV, FLAC or MP3. Has a waveform view and a gain slider. | (Source has no equivalent) |
| **Forge** | Compiles source meshes into engine models. | `studiomdl` |
| **Vault** (archive) | Packs a content tree into one archive. | `vpk` |
| **Kiln** (build) | Runs the whole pipeline over a project. | the batch file everyone writes |

The stages also run headless, as subcommands, for scripts and build servers:
`kerosene-tools cleave map.keromap`, `kerosene-tools kiln`, and so on.

The engine is the `kerosene` crate, and a game is a binary that depends on
it; `kerosene`, the stock runtime, is that with the stock game.

---

## Working on Kerosene itself

Everything above is for making a game. To build this repository — the
engine, the tools and the sample content — you need Rust 1.94 or later.

```sh
cargo build --release              # the engine and the toolset
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

Once the toolset is built, **`kerosene-tools kiln`** builds a project's
content — textures, models, maps, and the archive — from anywhere, and the
**Build tab** in the toolset window does the same with a button. It is a
program rather than a shell script because a script is not shipped: install
the toolchain somewhere and the thing that knows how to use it would stay
behind in a git checkout. `scripts/build-content.sh` is a wrapper that builds
the toolset from source and regenerates the sample map, then calls it.

To open the toolset window (the editor, on the way in):

```sh
cargo run --release -p kerosene-tools
```

To open a specific map in the editor:

```sh
cargo run --release -p kerosene-tools -- chisel content/maps/kero_start.keromap
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

The four stages are separate on purpose. Each reads and writes files, so you
can stop after any of them, run them from a Makefile, or parallelise them
across a build farm.

```sh
cleave    content/maps/kero_start.keromap    # → .kerobsp and .keroprt
umbra     content/maps/kero_start.kerobsp    # → adds visibility
resonance content/maps/kero_start.kerobsp    # → adds acoustics
radiance  content/maps/kero_start.kerobsp    # → adds lighting
```

An unvised, unlit map still loads and plays; it just draws everything, looks
flat and sounds dry. That is deliberate — you should be able to walk a level
thirty seconds after drawing it.

`umbra --fast`, `resonance --fast` and `radiance --fast` skip the expensive
passes while a layout is still moving.

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
  kerosene            the game crate: what a game depends on, and the API
                      Kerosene's version follows (see Versioning)
  kerosene-engine     the host: the simulation, with and without a window;
                      the Game trait; base/, the content every game starts with
  kerosene-game       the stock entity classes — the game DLL analogue
  kerosene-entity     entities, their fields, and the I/O event queue
  kerosene-physics    player movement and collision response
  kerosene-rigid      rigid-body props on Box3D (box3d-rust)
  kerosene-render     the wgpu renderer, lightmap atlas, PVS culling
  kerosene-ui         the game UI: layouts, stylesheets, store, UI scripts
  kerosene-script     Rhai map scripting
  kerosene-platform   the store: Steam, or nothing
  kerosene-audio      the mixer, spatial sound and reverb
  kerosene-anim       skeletal animation
  kerosene-math       vectors, planes, convex windings with exact clipping
  kerosene-kv         KeyValues, the text format .keromap and materials use
  kerosene-config     engine.kconfig — the settings every program shares
  kerosene-console    convars, concommands, the command buffer
  kerosene-vfs        layered search paths and the .vault archive format
  kerosene-asset      .kerotex textures, .keromat materials, .keromdl models
  kerosene-map        .keromap — the editable map format
  kerosene-bsp        .kerobsp — the compiled map, plus tracing and PVS
  kerosene-walk       walkable-surface data for navigation
tools/                published as kerosene-<tool>
  chisel cleave umbra resonance radiance alchemy forge timbre vault kiln loupe
  kerosene-tools      all of them as one application, and `new` and `play`
apps/
  kerosene            the stock runtime (package kerosene-runtime)
kerosene.keroproj     the project file: what content tree this is, and where
content/              sample art, models, materials, the sample level, and the
                      archive packed from them
src/                  the book: getting started, the game developer guide,
                      the engine's documentation and the devnotes
```

Read [`docs/architecture.md`](src/docs/architecture.md) for how the pieces fit,
[`docs/formats.md`](src/docs/formats.md) for the file formats, and
[`docs/tools.md`](src/docs/tools.md) for the full tool reference.
[`docs/scripting.md`](src/docs/scripting.md) covers the script API,
[`docs/audio.md`](src/docs/audio.md) sound,
[`docs/configuration.md`](src/docs/configuration.md) the engine config, and
[`docs/licensing.md`](src/docs/licensing.md) the dependency audit and the
provenance of the algorithms. [`docs/positioning.md`](src/docs/positioning.md)
argues what the engine is shaped to be good at, and
[`docs/missing-features.md`](src/docs/missing-features.md) inventories what it
does not have yet.

---

## Status

Kerosene is **1.0.0 alpha**: it works end to end, and its API may still
change between alphas. From `1.0.0` on, the `kerosene` crate follows
Semantic Versioning, and everything that counts as a breaking change is
written down in [Versioning](src/docs/versioning.md). What changed, release
by release, is in [`CHANGELOG.md`](CHANGELOG.md).

You can make a game with `kerosene-tools new`, draw its levels in Chisel,
compile them, and play, save and load them. More than two thousand tests
cover the pieces and the seams between them, including suites that build
maps in memory, compile them, load them and play them, and CI makes and runs
a new game on Linux, Windows and macOS.

Known limits, stated plainly:

- **No networking yet.** The engine is structured for a client/server split —
  the simulation runs without a display, which is the hard part — but the
  wire protocol and prediction are not written.
- **No combat, NPCs or particles yet.** The stock weapons are a starting
  point for the HUD, not a combat system. These are on the roadmap in
  [Missing features](src/docs/missing-features.md).
- **Chisel's 3D view is software-rasterised, not GPU-rendered.** Occlusion is
  correct — it has a real depth buffer — and it draws materials, mipped and
  perspective-correct. There is no lighting and there are no shadows. The
  compiled map in the engine is one keystroke away.
- **No block compression for textures.** `.kerotex` is uncompressed.
- **Not on crates.io yet.** Until the first release is published, `new`
  takes `--kerosene-git` or `--kerosene-path` to depend on a checkout.

## Licence

**GPL-3.0-or-later WITH the Kerosene Exception.** The full texts are
`LICENSE` and `LICENSE-EXCEPTION`, and every source file carries an
`SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0`
line.

The exception is what makes a game possible: it lets you link Kerosene,
statically or dynamically, into a game and ship the game under your own
terms — your code, assets and levels are yours, closed or open. In return
the engine part stays under the GPL with its source available, the game says
it is built with Kerosene and shows that on an attribution screen when it
starts, and a modified engine says "modified from Kerosene", names the
version it diverged from, and is published whole. The project's preference:
if you change Kerosene itself, contribute the change back as a pull request
rather than releasing a modified fork.
[`docs/licensing.md`](src/docs/licensing.md) explains all of this properly,
along with the full dependency audit and the provenance of the algorithms.
