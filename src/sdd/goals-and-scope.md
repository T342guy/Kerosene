# Goals and scope

## What Kerosene is

Kerosene is a Source-1-style game engine written in Rust: BSP levels built
from brushes, baked visibility, lighting and acoustics, entity I/O wiring, and
a fixed-rate simulation. It ships as **one published crate**, `kerosene`, that a
game depends on, plus the toolset (an editor and compilers) a game uses to make
its content.

The crate is the product. Its public surface is what the engine's version
number follows; see [Public API](public-api.md) and
[Positioning](../docs/positioning.md).

## Goals

1. **A game is a crate.** A game implements the `Game` trait, registers its own
   entity classes and calls `kerosene::launch`. The engine owns no game.
2. **Fast runtime, slow build.** Everything expensive (visibility, lighting,
   acoustics, texture and model compilation) happens in tools. The runtime loads
   compiled files only.
3. **A complete toolchain.** Level editor, compilers, asset tools and a
   whole-project build ship with the engine, so a game does not need to build
   its own.
4. **Headless by construction.** The simulation runs with no window, GPU or
   input device, so it can be tested and later hosted as a server.
5. **Stable where it matters.** One lockstep version, SemVer over the facade's
   stable modules.
6. **One declaration per fact.** An entity field is declared once and the map
   loader, saves, editor and scripts all read that declaration.

## Non-goals

- **Not a general-purpose engine.** There is no scene graph, no open-world
  streaming of arbitrary assets, no visual scripting. The shape of a level is a
  BSP of brushes.
- **No runtime compilation of source content.** `.kmap` source is linked only
  by tools.
- **No networking yet.** There is no client/server protocol, replication or
  prediction. The headless `Engine` is the groundwork. See
  [Status and roadmap](status-and-roadmap.md).
- **No migration tooling in alpha.** Breaking changes need no migrate command
  or shim until 1.0 or until real games depend on the engine. See the
  [decision log](decisions.md).

## Users

| User | Uses | Cares about |
|---|---|---|
| Game author | the `kerosene` crate, the tools | a stable API, content that builds and ships |
| Level designer | Chisel, the unified tools app | editor ergonomics, fast iteration |
| Engine contributor | the workspace | layering, tests, devnotes |

## Constraints

- Rust edition 2024, MSRV 1.94, toolchain pinned in `rust-toolchain.toml`.
- Linux, Windows and macOS; CI builds all three.
- GPL-3.0-or-later with the Kerosene exception, so games built on the engine
  are not forced to be GPL. See [Licensing](../docs/licensing.md).
- Units: 1 ku (Kerosene unit) = 1 inch, Z up.

## See also

- [Positioning](../docs/positioning.md)
- [Missing features](../docs/missing-features.md)
- [Licensing](../docs/licensing.md)
