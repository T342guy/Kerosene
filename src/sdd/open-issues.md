# Open issues

Inconsistencies and gaps found while writing this document (2026-09-30).
They are recorded here, not fixed. Remove an item when it is resolved.

## Documentation

| # | Issue | Where |
|---|---|---|
| O1 | Crate count is stale: the crate map says 23 engine crates and 39 packages; the workspace has 41 members (it omits `kerosene-ecs`, `kerosene-reflect`, `kerosene-toolui` from the count). | `src/devnotes/crate-map.md` |
| O2 | The crate map's layer table does not list `kerosene-ecs` and `kerosene-reflect`; `xtask/src/layers.rs` puts both in layer 1. It also omits layer 7 (the runtime binary). | `src/devnotes/crate-map.md` |
| O3 | `phase-0-audit.md` describes a 35-crate workspace; it is a point-in-time document and should say so. | `src/refactor/phase-0-audit.md` |
| O4 | `src/refactor/` and `src/docs/Devnotes/reworkConfig.md` are not linked from `SUMMARY.md`. | `src/SUMMARY.md` |

## Code and content

| # | Issue | Where |
|---|---|---|
| O5 | Stale `kero_start` references although the engine no longer ships that map (the fallback is `kerosene_room`): the `map` convar default, a doc example, several tests and a help string. | `crates/kerosene-console/src/lib.rs`, `crates/kerosene-engine/src/engine/config.rs`, `crates/kerosene-game/src/triggers.rs`, tests in `kerosene-vfs`, `kerosene-ui`, `kerosene-render`; also `src/docs/scripting.md`, `src/gamedev/steam.md` |
| O6 | The MSRV (`rust-version = 1.94`) differs from the pinned toolchain (1.98.1). Intentional, but untested: CI does not build on 1.94. | `Cargo.toml`, `rust-toolchain.toml` |
| O7 | `PhysicsProps` and texture/model loading still bypass the Phase 5 design. | see [Status and roadmap](status-and-roadmap.md) |
| O8 | `ui` → `script` is a recorded layering exception waiting for a better home for the shared Rhai code. | `xtask/src/layers.rs` |

## Demo repository

| # | Issue |
|---|---|
| O9 | `kerosene-demo` depends on `kerosene = "1.0.0-a2"` and uses old extensions (`.keromap`, `.keroproj`, `.keroscript`, `.kerosnd`); a4 uses `.kmap`, `.kproj`, `.kscr`, `.ksnd`. |

## Testing

| # | Issue |
|---|---|
| O10 | No test drives the real `launch` boot path. |
