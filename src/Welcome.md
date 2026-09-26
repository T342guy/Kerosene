# Welcome!

Hello there! Thank you for checking out Kerosene.

![Kerosene](./Images/kerosene-readme-banner.png)

> [!NOTE]
> AI Generated content disclosure: Most of these docs were made by AI. If you want to improve them, please do! All help to kerosene is greatly appreciated :>

> [!WARNING]
> Kerosene is 1.0.0 alpha. It works end to end, but its API may still change
> between alphas, and a finished game still needs things it does not have
> yet: see [Missing features](docs/missing-features.md).

Hello! Welcome to the Kerosene documentation.

Have you ever used the Source engine? Have you wished it was open source and you could make a game with the same tools? With Kerosene, we've tried our best!

Kerosene is a **Rust game crate** for brush-built 3D games. You add `kerosene`
to a Cargo project, implement one trait, and you have a game, with the
editor and every compiler along with it. Everything builds from source on
your own machine, so the same game builds on Linux, Windows and macOS.
It is PURELY open source: GPL 3.0 with an exception that lets your game stay
yours, so long as it says it is built with Kerosene.

```sh
cargo install kerosene-tools
kerosene-tools new mygame
cd mygame
cargo play
```

Kerosene has, among other things:

> [!TIP]
> This list is incomplete, please read the rest of the docs to get a full view of this engine's capabilites.

- A Source-style movement solver, air-strafing and all
- Brush-based world geometry, compiled to a BSP tree with baked visibility, lighting and acoustics
- Physics props on Box3D
- Entity I/O, Rhai map scripting and a game UI
- Skeletal animation
- Saved games and level changes
- Steam: achievements, stats, leaderboards, cloud saves and the Workshop
- Chisel, a level editor, and a compiler for every kind of content
- And more!

**Making a game?** Start with [Getting started](gamedev/getting-started.md),
then [Making a game](gamedev/making-a-game.md), and read
[Publishing](gamedev/publishing.md) before you hand anything to anyone.

**Working on Kerosene itself?** The [Documentation](docs/architecture.md)
and the [Devnotes](devnotes/README.md) are how it works inside.
