# Decision log

Each entry: what was decided, why, and what it costs. Append new decisions at
the end; do not rewrite old ones. If a decision is reversed, add a new entry
that supersedes it and mark the old one.

Status values: **Accepted**, **Revisit**, **Superseded**.

---

### D1. The `kerosene` crate is the product

**Status:** Accepted

One published crate; one lockstep version is its API under SemVer; tags are the
bare version. Internal crates are `publish = false` and folded into it by
`cargo xtask bundle`.

*Why:* a game author wants one dependency and one version to reason about.
*Cost:* the bundle step rewrites paths, so the published crate's layout differs
from the workspace.

### D2. Layered dependencies, enforced

**Status:** Accepted

Crates depend only downward; subsystems never on each other; checked by
`cargo xtask layers`.

*Why:* see [Principles](principles.md). *Cost:* sharing between subsystems means
moving code down a layer, which is sometimes awkward. One exception exists
(`ui` → `script`) and is marked **Revisit**.

### D3. Bake at build time

**Status:** Accepted

Visibility, lighting and acoustics are tools, not runtime systems.

*Why:* predictable frame time; a small runtime. *Cost:* longer content builds,
no fully dynamic worlds.

### D4. `bevy_ecs` and `bevy_reflect` behind facades, exact-pinned

**Status:** Accepted

Used for entity storage and field reflection, reached only through
`kerosene-ecs` and `kerosene-reflect`, pinned with `=`.

*Why:* a mature ECS and reflection without handing games Bevy's API surface or
churn. *Cost:* upgrades are deliberate work.

### D5. wgpu and egui versions move together

**Status:** Accepted

`wgpu` is pinned to the version `egui-wgpu` links (25 with egui 0.32); other
crates reach wgpu through `kerosene_rhi::wgpu`.

*Why:* two wgpu versions in one binary do not interoperate. *Cost:* wgpu
upgrades wait for egui.

### D6. Rhai for scripting

**Status:** Accepted

*Why:* pure Rust, sandboxable, no build-system burden, easy to give a snapshot
and collect actions. *Cost:* slower than a JIT language, a small ecosystem.

### D7. Headless `Engine`, windowed `host`

**Status:** Accepted

See [Runtime](runtime.md). *Why:* servers, CI, tests. *Cost:* every host
concern needs a seam rather than a direct call.

### D8. Request-shaped handlers

**Status:** Accepted

Entity handlers and console commands leave requests rather than call into the
engine.

*Why:* keeps those crates plain data, testable alone. *Cost:* an indirection,
and a verb nobody claims is only caught at run time (it is reported, not
silent).

### D9. Engine owns no game; fallback map only

**Status:** Accepted

The engine ships a built-in fallback map, `kerosene_room`, and base content,
not a demo game. The demo game (`kero_start`) lives in the separate
`kerosene-demo` repository as an ordinary crates.io game crate.

*Why:* proves a game really is a crate, and keeps the engine free of game
content. *Cost:* the demo can lag the engine (see [Open issues](open-issues.md)).

### D10. No migration support in alpha

**Status:** Accepted until 1.0, or until real games exist

Breaking changes (for example removing the `func_` prefix from class names, in
1.0.0-a4) ship with no migrate command or compatibility shim; the
`CHANGELOG.md` says what to change.

*Why:* nothing yet depends on the old shapes, and shims are cost with no user.
*Cost:* old maps and `.kdef` files need hand edits.

### D11. Short file extensions

**Status:** Accepted

Formats use short `k…` extensions (`.kmap`, `.kbsp`, `.kproj`, …).

*Why:* consistency and brevity. *Cost:* older material (the demo's README, some
docs) may still show long names.

### D12. Source-style movement, separate from rigid bodies

**Status:** Accepted

*Why:* the movement feel is the point of a Source-style engine, and it must be
deterministic and BSP-trace-based. Rigid props use a general solver
(`box3d-rust`) and couple to the player explicitly. *Cost:* two collision
representations to keep consistent.

### D13. License with an exception

**Status:** Accepted

GPL-3.0-or-later with the Kerosene exception. See
[Licensing](../docs/licensing.md).

---

## Template

```markdown
### Dn. Title

**Status:** Accepted | Revisit | Superseded by Dm

What was decided.

*Why:* …  *Cost:* …
```
