# Subsystems

Each subsystem is a layer-3 crate (or small group) that depends only downward
and never on another subsystem. The engine in layer 4 is where they meet. Each
section gives the design decision; the devnote has the internals.

## Rendering

**Crates:** `kerosene-rhi`, `kerosene-scene`, `kerosene-material`,
`kerosene-render`.

- **Split by role.** `rhi` opens the GPU and surface (the only crate that
  does); `scene` is the CPU-side description of *what to draw* (camera, world
  mesh, lightmaps, lights, probes, decals, UI display list); `material` holds
  materials on the GPU; `render` is the wgpu renderer.
- **Why a scene contract.** The UI and tools can produce draw data without a
  GPU, and tests can build and inspect it without one. A software rasteriser
  remains for tool tests.
- **World drawing.** PVS-culled, with lightmaps baked at build time, dynamic
  lights and probes layered on top. The world is divided into **sections** that
  stream in on worker threads and upload on the main thread.

Detail: [Rendering and streaming](../devnotes/rendering.md).

## Physics

**Crate:** `kerosene-physics`.

- **Player movement is Source-style** (`gamemovement`), deterministic and
  separate from rigid bodies, using BSP traces through the `CollisionWorld`
  trait.
- **Props are rigid bodies** in a `PhysicsWorld` backed by `box3d-rust`.
  Static world sections are added as hulls as they stream in.
- **Player and props couple explicitly:** the player's box is synced into the
  rigid world before the step (props bounce off) and the player's *requested*
  direction pushes props (leaning on a crate works).

Detail: [Physics](../devnotes/physics.md).

## Audio and acoustics

**Crate:** `kerosene-audio`.

- A pure-Rust mixer with spatial sound and an FDN reverb; `cpal` output is
  behind the `audio` feature.
- Acoustics are **baked** by `resonance` into the `.kbsp`; at run time the
  engine picks reverb parameters from the listener's position.

Detail: [Audio and acoustics](../devnotes/audio.md), [Audio](../docs/audio.md).

## Animation

**Crate:** `kerosene-anim`. Skeletons, clip sampling, crossfade and a skinning
palette. NPC behaviour is not built (see
[Status and roadmap](status-and-roadmap.md)).

## Scripting

**Crate:** `kerosene-script`. Rhai, one VM per map, world snapshot in and
action queue out. The single `rhai` dependency lives here; the game UI is the
one other user of it (the recorded `ui` → `script` exception).

## Game UI

**Crate:** `kerosene-ui`. XML layout (`.kui`), CSS-like style (`.kcss`), Rhai
behaviour (`.kscr`), a data store with bindings, and flexbox layout through
`taffy`. It produces a display list in the `scene` contract; the renderer draws
it. egui is for tools and debug overlays, not for game UI.

Detail: [The game UI](../devnotes/ui.md), [Game UI](../docs/ui.md).

## Platform (store)

**Crate:** `kerosene-platform`. A `Platform` (validation, batching, events)
over a `Backend`: Steam behind the `steam` feature, an offline stand-in
otherwise. Achievements, stats, cloud saves and Workshop all go through it.
Games never call Steam directly.

Detail: [The store (Steam)](../devnotes/platform.md), [Steam](../gamedev/steam.md).

## Console and configuration

**Crates:** `kerosene-console`, `kerosene-config`. Convars and commands are the
engine's control surface: input binds commands, config files are command
scripts, and a game adds its own in `setup`. `engine.kcfg` holds persistent
engine settings such as the renderer choice.

## See also

- [Devnotes index](../devnotes/README.md)
- [Documentation](../docs/architecture.md)
