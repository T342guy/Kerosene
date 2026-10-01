# Design principles

Three rules govern the whole engine. Each is enforced, or should be, by
something other than good intentions.

## 1. Nothing points upward

Crates sit in layers and depend only on the layers below. Subsystems (render,
physics, audio, entities, scripting, UI) do **not** depend on each other; when
two need to share something, it moves down a layer or the engine passes it
across.

*Why:* it keeps each subsystem testable alone, and stops the engine growing a
tangle where changing audio breaks rendering.

*Enforced by:* `cargo xtask layers`, which reads the table in
`xtask/src/layers.rs` and runs in CI. A new crate is an error until it is
given a layer. See [Architecture](architecture.md).

## 2. Everything expensive happens at build time

Source content is compiled by tools into formats the runtime reads directly.
Visibility, lighting and acoustics are baked. The runtime does no CSG, no
lightmap solving and no format conversion.

*Why:* it makes the runtime small and its frame time predictable, and gives a
clear line between "the editor" and "the game".

*Enforced by:* `kerosene-map` (the `.kmap` source format) may only be linked at
layer 5 or above. See [Data and the build pipeline](data-and-pipeline.md).

## 3. The engine owns no game

The engine defines the `Game` trait and the machinery around it. Doors,
triggers, weapons and the rest are a *game*, and the stock set lives in
`kerosene-game` as one implementation among possible others. `kerosene-engine`
does not depend on it.

*Why:* a game must be able to replace any of the rules, not work around them.

*Enforced by:* the dependency graph: `kerosene-game` is a dev-dependency of the
engine for tests only. See [Runtime](runtime.md).

## Supporting conventions

| Convention | Rule | Reason |
|---|---|---|
| Fixed tick | Simulation runs at `sv_tickrate` (default 64 Hz); rendering interpolates | Deterministic gameplay, smooth display |
| Request-shaped I/O | Handlers leave a `HostRequest`/console request instead of reaching into the engine | Keeps entities and console plain data, testable without an `Engine` |
| Declare once | Fields carry reflection attributes (`Key`, `Help`, `Label`, …) | One source for loader, saves, editor, scripts |
| Facade over dependencies | `bevy_ecs`, `bevy_reflect`, `wgpu` reached through our own crates | The pinned third-party version can change without a public API change |
| One version | Every crate shares a version, pinned with `=` | No skew between crates |
| Units | 1 ku = 1 inch, Z up | One convention for tools and runtime |

## See also

- [Crate map](../devnotes/crate-map.md)
- [Architecture](../docs/architecture.md)
- [Refactor design document](../refactor/phase-0-audit.md) (the audit that set these rules)
