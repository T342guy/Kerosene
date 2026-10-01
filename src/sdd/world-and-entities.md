# World and entities

## Two kinds of world data

| | Static world | Entities |
|---|---|---|
| Held by | `kerosene-bsp` (`.kbsp`) | `kerosene-entity` (`EntityWorld`) |
| Changes at run time | no (sections stream in and out) | yes |
| Used for | traces, visibility, rendering, acoustics, navigation | everything that has behaviour |

Brush *entities* (doors, platforms, triggers) are brushes the map compiler
splits out of the static world and attaches to an entity; their poses are
dynamic and interpolated per frame.

## Entities on `bevy_ecs`

`EntityWorld` stores entities as ECS components, with `bevy_ecs` reached
through the `kerosene-ecs` facade. Entities are addressed by Source-style
names and slot handles so that map files, scripts and saves can refer to them.

### Declared once

A class's fields are declared once on a Rust type with reflection attributes
from `kerosene-reflect`:

```rust
#[reflect(@Key("speed"), @Help("Units per second"))]
speed: f32,
```

The map loader (KeyValues → fields), the save snapshot, the Chisel entity
inspector and scripts all read that declaration. Other attributes: `Label`,
`Hidden`, `Widget`, `Networked`, `Transient`. `Networked` is recorded but no
code reads it yet (see [Status and roadmap](status-and-roadmap.md)).

### Classes

A class is registered with a `ClassRegistry`. A `.kdef` file describes it for
the tools: keys, defaults, inputs, outputs and `helper { … }` blocks that tell
Chisel how to draw it. The stock classes (`kerosene-game`) are an ordinary
registration, not special.

## World geometry types

Brush entity class names carry no prefix (the `func_` prefix was dropped in
1.0.0-a4). Among them: `door`, `door_rotating`, `detail`, `brush`, `button`,
`ladder`, `breakable`, `wall_toggle`, `rotating`, `illusionary`, `areaportal`,
`occluder`, `water`, `liquid`, `movelinear`, `platform`, `tracktrain`,
`physbox`. There is no compatibility shim: old maps and `.kdef` files need the
same rename. The list in `CHANGELOG.md` (1.0.0-a4, *Changed*) is authoritative.

## Entity I/O

Level logic is wired with Source-style connections: *output → target.input
(parameter, delay, fire-count)*. The connection encoding lives in
`kerosene-kv`; delivery is an ordered event queue in `kerosene-entity`.
Handlers receive `&mut EntityWorld` only; anything wider is a `HostRequest`
answered by the engine or the game (see [Runtime](runtime.md)). This keeps
`kerosene-entity` testable without an engine.

## Scripting

Per-map `.kscr` scripts run in Rhai (`kerosene-script`). Scripts see a snapshot
of the world and return a queue of actions (`ScriptAction`) the engine applies,
so a script cannot reach engine state directly. Entity I/O remains the primary
level-logic mechanism; scripts are for what wiring cannot express. Hooks (for
example `on_tick`) are optional.

## Saves

`crates/kerosene-engine/src/save.rs` writes `.ksav` as JSON: entity snapshots
(everything not `Transient`), player state, the map, and the game's own data
through `Game::save`/`load`. Level changes carry state across maps. See
[Saving and level changes](../gamedev/saving.md).

## See also

- [Entities and scripting](../devnotes/entities-and-scripting.md)
- [Scripting](../docs/scripting.md)
- [Saving and level changes](../gamedev/saving.md)
