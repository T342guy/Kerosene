# Getting started

Kerosene is a crate. A game is a Cargo package that depends on it, and
everything — the engine, the editor, the compilers, your game — builds from
source on your own machine. There is no installer, no launcher and no
account. What runs on Linux runs on Windows and macOS, because it is the same
code compiled there.

## What you need

- **Rust 1.94 or later**, from [rustup](https://rustup.rs).
- **On Linux**, the ALSA headers for sound: `libasound2-dev` on Debian and
  Ubuntu, `alsa-lib-devel` on Fedora. Without them, a game still builds with
  `--no-default-features`, just silently.
- A GPU with Vulkan, Metal or DirectX 12. The editor's 3D view is drawn in
  software and needs none.

## Five minutes to a running game

```sh
cargo install kerosene-tools
kerosene-tools new orbital-drift --name "Orbital Drift"
cd orbital-drift
cargo play
```

> [!NOTE]
> Until the first crates.io release, install the toolset from the
> repository instead:
> `cargo install --git https://github.com/t342guy/kerosene kerosene-tools`.
> A game made by that toolset depends on the matching release tag, so it
> builds the same Kerosene the toolset is.

`new` makes this:

```text
orbital-drift/
  Cargo.toml              the package: kerosene, and two binaries
  .cargo/config.toml      cargo play, cargo tools, cargo ship
  orbital-drift.keroproj  the project: its name, content, start map, game
  src/
    main.rs               the game binary: launch(game, options)
    game.rs               the game: a Game, and a class of its own
    tools.rs              the game's own editor and compilers
  content/
    maps/orbital_drift_start.keromap
    art/ materials/ models/ sound/ scripts/ textures/
```

The first `cargo play` builds the engine and its tools, which takes a few
minutes. Then it compiles the starter map and opens the game on it. Walk
up to the orange plinth: the gem on it is an `item_pickup`, a class the game
defines in `src/game.rs`, and the console (`` ` ``) says so when you take
it. F5 saves, F9 loads.

Every `cargo play` after that rebuilds only what changed. With nothing
changed, the game starts at once.

## The loop

| Command | Does |
|---|---|
| `cargo play` | Build changed content, build the game, run it on the start map |
| `cargo play +map other` | ...on another map. Anything after `play` goes to the game. |
| `cargo play --full` | Build maps with full visibility and lighting first |
| `cargo tools` | The toolset window: project, editor, sound editor, build, archive |
| `cargo tools chisel content/maps/x.keromap` | The editor, on a map. F9 in it compiles the map and plays it. |
| `cargo ship` | Build everything properly and assemble `dist/`, ready to hand out |
| `cargo run` | The game on its own, with the content as it is |

`cargo play` skimps the maps' visibility and lighting, because a layout
you are still moving walls around in does not need them. `cargo ship`
always does them properly.

## Where things go

**Your code** is `src/game.rs` and whatever modules you add. A game is one
type implementing `Game`; every method has a default, so you add the ones
you need. [Making a game](making-a-game.md) goes through each.

**Your content** is `content/`. Maps are drawn in Chisel. Textures go in
`art/` as PNGs; models in `art/` as OBJ or glTF; sounds in `sound/` as WAV,
FLAC or MP3. `cargo play` compiles each into what the engine reads, beside
its source.

**The engine's base content** is under all of it. Kerosene carries the
developer textures, the stock props, sounds and UI, and a demo map, compiled
into the engine, so a game has something to show before it has anything of
its own. Put a file at the same path in `content/` and yours is used
instead: `content/ui/hud.keroui` replaces the stock HUD.

## Another platform

Build on it. Clone your game on the Windows or macOS machine and run the same
three commands; nothing in a Kerosene game is specific to where it was
started. `cargo ship` there makes that platform's `dist/`.

`kiln --ship dist --target <triple>` cross-compiles, for a developer who has
set up that target's linker and libraries. That setup is Rust's rather than
Kerosene's, and building on each platform is the supported way.

## Staying up to date

Kerosene follows [Semantic Versioning](../docs/versioning.md) on the
`kerosene` crate's API. Your `Cargo.toml` names a version; `cargo update`
takes fixes and additions without breaking your game, and a new major
version is one you move to on purpose, with the
[changelog](https://github.com/t342guy/kerosene/blob/MASTER/CHANGELOG.md)
saying what to change.

## Next

- [Making a game](making-a-game.md): the `Game` trait, classes, the schema
  and your game's tools.
- [Saving and level changes](saving.md).
- [Publishing](publishing.md), before you hand anything to anyone.
