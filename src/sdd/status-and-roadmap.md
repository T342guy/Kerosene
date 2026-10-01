# Status and roadmap

Current version: **1.0.0-a4** (2026-09-28). Alpha: the API is not yet promised.

## Refactor phases

The architecture refactor (studied from Source 2; see
[Phase 0 audit](../refactor/phase-0-audit.md)) ran in phases.

| Phase | Work | State |
|---|---|---|
| 0 | Audit against the checklist | Done |
| 1 | Layer the dependency graph; add `cargo xtask layers`; remove unused edges | Done |
| 2 | Compiled resource container and `Resource<T>` handles | Done |
| 3 | Reflection (`kerosene-reflect`) | Done |
| 4 | ECS (`bevy_ecs` behind `kerosene-ecs`); stock classes ported | Done |
| 5 | Split render and physics (`rhi`, `material`, `scene`, `PhysicsWorld`; engine/host split) | Done, with leftovers |

### Leftovers

- `PhysicsProps` still lives in the engine and owns the entity-to-body mapping.
- Render texture and model loading do not yet go through `Resources`.
- `.kmat` runtime fallback is incomplete.
- `kiln --force` does not reach the texture pass.

## Feature status

| Area | State |
|---|---|
| Fixed tick, interpolation | Done |
| BSP, PVS, baked lighting and acoustics, streaming | Done |
| Entity I/O, Rhai scripting | Done |
| Game UI (`.kui`/`.kcss`/`.kscr`) | Done |
| Saves and level changes | Done |
| Steam (achievements, stats, cloud, Workshop) | Done, optional feature |
| Unified tools app, Chisel on GPU | Done |
| MSAA, CI | Done |
| Demo record and playback | Not started |
| Chisel instances | Not started |
| Weapons and damage (hitscan, ammo, decals) | Not started |
| Impact sounds | Not started |
| Animation-driven NPCs | Not started |
| Networking (protocol, replication, prediction) | Not started |

The authoritative gap list and its suggested order are in
[Missing features](../docs/missing-features.md); this table is a summary and
may lag it.

## Path to 1.0

- Close the phase 5 leftovers.
- Decide the stable API surface (the `internals` boundary) with at least one
  real game depending on it.
- Pick the next feature work from [Missing features](../docs/missing-features.md).
- Bring the demo game up to date with the current formats.
- Resolve [open issues](open-issues.md).

Networking is the largest unbuilt piece. The design groundwork is the headless
`Engine` and the `Networked` field attribute; Steam Datagram Relay is the
suggested transport.

## See also

- [Missing features](../docs/missing-features.md)
- [Phase 0 audit](../refactor/phase-0-audit.md)
- `CHANGELOG.md` at the repository root
