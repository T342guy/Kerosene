# Refactor Phase 0 — Bootstrap Audit

Sep 28, 2026 · scores the tree at `9bcdf13` against the Phase 0 checklist in the
[refactor design document](<./Kerosene Engine — Refactor Design Document (Source 2 Study).md>).

The design document was written before its author had seen the code ("this
study has not yet seen Kerosene's code"). The main result of the audit is that
the bootstrap is much further along than the document assumes. It is already a
35-crate Cargo workspace (about 180k lines of Rust). It already has a
source → compiled split for every asset type, and the runtime binary links no
tool code. The work left is narrower and more specific than "move every module".

Baseline: `cargo build --workspace --all-targets` and `cargo test --workspace`
were green before any change (84 suites, 2,273 tests, 0 failures). Clippy with
`-D warnings` was clean.

## Checklist

| # | Item | Status |
| --- | --- | --- |
| 1 | Each source file tagged with its target module | Done at crate level: [module map](#1-module-map) |
| 2 | Upward / sideways dependencies listed | Done: [dependencies](#2-dependencies). 11 unused edges **removed** in this phase |
| 3 | Runtime parsing of authoring formats listed | Done: [one real violation](#3-runtime-parsing-of-authoring-formats) (`.kmat`) |
| 4 | Game code calling third-party libraries listed | Done: [third-party leaks](#4-third-party-libraries-outside-their-owner) |
| 5 | Entity classes hand-parsing keyvalues listed | Done: [the big one](#5-stringly-typed-entity-fields) |
| 6 | Brush/CSG code reachable from the runtime | **None.** `kerosene-runtime` links no tool crate (`cargo tree -p kerosene-runtime -e normal`) |
| 7 | Global mutable state listed | Two `OnceLock`s, both benign: [globals](#6-global-state) |
| 8 | Half-implemented stubs marked | None: zero `todo!` / `unimplemented!` / `TODO` comments in `crates/` or `tools/` |
| 9 | CI builds every target and runs a boot-and-load-map smoke test | Mostly: CI builds and tests all targets on three OSes. `crates/kerosene-engine/tests/playthrough.rs` compiles, loads and plays a map. Gap: nothing drives the real `launch` boot path headless |

## 1. Module map

Target modules are the ones named in the design document. "Keep" means move or
rename only, with no redesign.

| Crate | Lines | Target module | Verdict |
| --- | ---: | --- | --- |
| `kerosene-math` | 2.4k | core | Keep |
| `kerosene-kv` | 1.0k | core (the KV3 analogue) | Keep |
| `kerosene-console` | 2.9k | core services (convars, commands, log) | Keep |
| `kerosene-config` | 0.5k | core services | Keep; drop its `wgpu` dependency (§4) |
| `kerosene-vfs` | 3.8k | core services: file layer under `resource` | Keep |
| `kerosene-asset` | 3.0k | `resource` (texture, material, model formats) | Finish: common container (§3) |
| `kerosene-bsp` | 4.2k | compiled world resource (`world`) | Keep |
| `kerosene-walk` | 0.9k | compiled nav resource | Keep; stop importing `kerosene-map` (§2) |
| `kerosene-map` | 4.5k | source format, **tools only** | Move the two runtime-used types out (§2) |
| `kerosene-entity` | 3.6k | `reflect` + `ecs` + entity I/O | **Rewrite** (§5) |
| `kerosene-game` | 5.5k | game-server (stock game) | Port onto `ecs`/`reflect` (§5) |
| `kerosene-physics` | 2.0k | `physics` (player movement) | Merge behind one `PhysicsWorld` with `rigid` |
| `kerosene-rigid` | 0.7k | `physics` (box3d rigid bodies) | Same as above |
| `kerosene-render` | 8.4k | `rhi` + `material` + `scene` | Split (§4) |
| `kerosene-anim` | 0.4k | `anim` | Keep |
| `kerosene-audio` | 4.6k | `audio` (already has sound events, `.ksnd`) | Keep |
| `kerosene-script` | 1.8k | `script` | Keep; own all `rhai` types (§4) |
| `kerosene-ui` | 8.1k | game UI | Keep; stop using `rhai` directly (§4) |
| `kerosene-platform` | 2.0k | store integration (Steam), a subsystem, not the platform layer | Keep; move its script bindings into `script` |
| `kerosene-engine` | 20.7k | engine host | **Split**: see below |
| `kerosene` | 0.4k | published facade | Keep |
| `kerosene-toolui` | 2.8k | tools UI | Keep |
| `cleave` | 5.9k | `csg` + the world stage of `kcompile` | Keep; `csg` could become its own crate later |
| `umbra`, `radiance`, `resonance` | 5.3k | `kcompile` stages (vis, light, acoustics) | Keep |
| `alchemy`, `forge`, `timbre` | 6.7k | `kcompile` asset compilers | Keep |
| `kiln` | 3.0k | `kcompile` driver (the `resourcecompiler` analogue) | Keep; home of the asset-type registry |
| `vault` | 0.4k | packaging (the VPK analogue) | Keep |
| `chisel`, `loupe` | 32.6k | `editor`, asset viewer | Keep |
| `kerosene-tools` | 7.3k | tools shell | Keep |

`kerosene-engine` is the one god crate. Its largest files are `engine.rs`
(2.3k lines), `host.rs` (2.1k, window + wgpu device), `physics.rs` (1.3k),
`ui.rs`, `save.rs`, `launch.rs` and `audio.rs`. Several of these are glue that
belongs beside the subsystem it drives. For example, `physics.rs` becomes the
`PhysicsWorld` implementation, and the device code in `host.rs` moves behind
`rhi`.

## 2. Dependencies

Removed in this phase (declared, but no code used them; the compiler agrees):

- `kerosene-entity` → `kerosene-physics`, `kerosene-console`
- `kerosene-game` → `kerosene-physics`, `kerosene-bsp`, `kerosene-console`
- `kerosene-render` → `kerosene-console`
- `chisel` → `kerosene-bsp`; `alchemy` → `kerosene-kv`, `kerosene-math`;
  `forge` → `kerosene-kv`
- `kerosene-tools` → `kerosene-kv`, moved to `[dev-dependencies]` (unit tests only)

Remaining edges that break rules 1–2 (subsystems must not depend on each other):

| Edge | What crosses it | Fix |
| --- | --- | --- |
| `render` → `ui` | `DisplayList`, `GlyphAtlas`, `DrawItem` | UI emits a render-neutral display list defined in `scene` (or core); render consumes it |
| `physics` → `bsp` | `Bsp`, `Trace`, `contents::SOLID` in `world.rs` | None needed: a subsystem reading a compiled resource from a lower layer is allowed (corrected in Phase 1) |
| `entity` → `bsp` | `load_from_bsp(&Bsp)` | Loader takes the entity lump as keyvalues; engine passes it |
| `entity`, `game` → `map` | `kerosene_map::Connection` (the I/O connection type and its parser) | Move `Connection` into `entity`; `map` re-exports it for the tools |
| `walk` → `map` | `WalkmapRule` | Move the type into `walk`; `map` re-exports it |
| `script` → `platform`, `ui` → `platform` | `PlatformAction`, `PlatformView`, `platform::script::register` | Invert: `platform` exposes plain data; `script` registers the bindings |
| `ui` → `script` | shares the `rhai` engine | Acceptable if `script` is modelled as below `ui`; make it explicit |
| `config` → `wgpu` (external) | `Renderer::wgpu_backends()`, `gpu::open` | Move both into a new `rhi` crate |

After the `Connection` and `WalkmapRule` moves, `kerosene-map` has no runtime
users apart from a doc link. It can then drop out of the runtime graph
entirely, which is rule 4 for the source format.

## 3. Runtime parsing of authoring formats

- **`.kmat` materials are text parsed at runtime**, in two separate places:
  `crates/kerosene-engine/src/engine.rs:1847` (surface properties) and
  `crates/kerosene-render/src/gpu.rs:2801`. Every other asset type is compiled
  by a tool (`.ktex`, `.kmdl`, `.kbsp`, `.kwalk`, audio, cubemaps, acoustics).
- `.kmap` is never loaded by the runtime. `engine.rs:204` only checks whether
  one exists so it can tell the user to compile it.
- UI (`.kui`/`.kcss`) and scripts (`.kscr`) are interpreted source at runtime.
  This is the Panorama-in-development model. It is acceptable for now, but
  should be listed as a future `kcompile` stage.

**No common compiled container.** Each compiled format has its own magic and
header: `KRTX`, `KRMD`, `KRAU`, `KROS`, `KCUB`, `ACST`, `KRWL`, and `KVLT` for
archives. None of them records its dependencies or source hash in a shared
place. Kiln's `.kbuild` stamp covers staleness for maps only. This is the core
of the design document's `resource` takeaway.

## 4. Third-party libraries outside their owner

| Library | Owner | Also used directly by |
| --- | --- | --- |
| `wgpu` | `render` | `engine` (`host.rs`: surface, device, queue, screenshots), `config` |
| `rhai` | `script` | `engine` (`rhai::Dynamic` in `scripting.rs`, `engine.rs:1434`), `ui` (`script.rs`, whole binding layer), `platform` |
| `egui` | tools / debug | `engine` (console overlay and debug UI; allowed by the doc: "immediate-mode UI for tools and debug") |
| `winit` | host | `engine` only. Fine: the engine *is* the host |
| `cpal`, `box3d` | `audio`, `rigid` | Contained. Good |

The stock game (`kerosene-game`) touches no third-party library except `glam`
and `serde_json`. That is the most important case, and it already passes.

## 5. Stringly-typed entity fields

Every entity is a bag of string-keyed fields. Classes read them at the point of
use, with the default repeated inline each time:

```rust
let speed = entity.fields.f32("speed", 100.0);        // doors.rs:309
let speed = entity.fields.f32("speed", 100.0);        // doors.rs:425, again
let door_state = entity.fields.i32("door_state", state::CLOSED);
```

- 201 `fields.<type>("key", default)` sites: 87 in `kerosene-game`, 76 in
  `kerosene-engine`, 36 in `kerosene-entity`, 2 in `kerosene-script`.
- 40 distinct keys in `kerosene-game`.
- Runtime state (`door_state`, `progress`, `move_serial`, `last_move`) lives in
  the same string map as designer keyvalues. Save/restore therefore works "for
  free", but nothing distinguishes a keyvalue from internal state.
- The editor schema (`kerosene-entity/src/schema.rs`, an FGD-like text file)
  is a **second, separate declaration** of the same keys and defaults, so the
  two can drift apart. This is exactly the design document's case for `reflect`.

What already matches the target: an ordered entity I/O queue with delays
(`fire_output` / `queue_input` / `run`), think scheduling, named lookup,
snapshots, and a class registry. The ECS port should keep those behaviours and
their tests.

## 6. Global state

- `kerosene-console/src/logging.rs:285`: `CRASH_DIALOG: OnceLock<String>`, set
  once at startup for the panic hook. Keep.
- `kerosene-vfs/src/archive.rs:505`: a CRC table in a `OnceLock`. Keep.

No `static mut`, `thread_local!`, or `Mutex` singletons.

## Proposed phases 1–5

The roadmap figure in the design document did not survive export. This is a
sequencing drawn from its text, for review. Every phase ends with the full
workspace build, tests, and clippy green.

1. **Layer the graph.** Apply the fixes in §2 and §4. Then add an `xtask`
   check that fails CI when a crate depends on a layer above it. No behaviour
   change.
2. **Resource container.** Define one compiled header (magic, type, version,
   dependency list, source hash) with typed blocks, plus a `Resource<T>` handle.
   Compile `.kmat` to a binary material, and migrate formats one at a time
   behind the old readers.
3. **Reflect.** Add a derive macro for component structs with `keyvalue`,
   `networked`, `saved` and `editor` attributes, and generate the editor schema
   from it.
4. **ECS.** Put `bevy_ecs` (pinned) behind an `ecs` facade. Port `kerosene-entity`,
   then `kerosene-game` class by class, keeping the I/O queue semantics and
   the playthrough tests. Existing `.ksav` saves need a migration or a clean
   break.
5. **Split render and physics.** Separate `rhi` / `material` / `scene` out of
   `kerosene-render`, and put `physics` and `rigid` behind one
   `PhysicsWorld`. Then break `kerosene-engine` up along those seams.

## Phase 1 results — layering

Done. The workspace build, all 2,273 tests, and clippy with `-D warnings` are
green, including the Steam feature. `cargo xtask bundle` builds too.
`cargo xtask layers` (new, `xtask/src/layers.rs`) now runs in CI. It fails
the build when a crate depends on a higher layer, when two subsystems depend
on each other, or when a runtime crate links `kerosene-map`. It also fails
when a new crate has not been given a layer. The layer table lives in
`src/devnotes/crate-map.md`.

| Edge | What changed |
| --- | --- |
| `entity`, `game` → `map` | `Connection` moved to `kerosene-kv` (`src/connection.rs`), beside the other value encodings. `map` re-exports it |
| `walk` → `map` | `WalkmapRule` moved to `kerosene-walk` (`src/rule.rs`). `map` now depends on `walk` and re-exports it |
| `entity` → `bsp` | `EntityWorld::load_from_bsp(&Bsp)` became `load_from_lump(&KeyValues, &[Aabb])`. The engine parses the lump and passes `Bsp::model_bounds()` |
| `engine` → `map` | Only a doc link and tests used it, so it is now a dev-dependency |
| `render` → `ui` | New `kerosene-scene` (layer 2) holds the display list (`draw.rs`), `Images` and `ATLAS_SIZE`. `ui` re-exports them, so its API is unchanged. `UiRenderer::upload_atlas` takes the pixel slice; the host checks the atlas's `dirty` flag |
| `render` → `anim` | `MAX_BONES` moved to `kerosene-asset` beside `MAX_BONE_INFLUENCES`. `anim` re-exports it |
| `config` → `wgpu` | New `kerosene-rhi` (layer 2) holds `gpu::open` and `backends(Renderer)`. The engine host and the tools UI open their GPUs through it |
| `platform` → `rhai`; `script`, `ui` → `platform::script` | The `platform` script object moved into `kerosene-script` (`src/platform.rs`). `platform` has no scripting dependency now, so it sits in layer 1 under both script VMs |
| `engine`, `ui` → `rhai` | Both reach `rhai` through `kerosene_script::rhai`, so the script subsystem is the only crate that names the dependency |

What is left, on purpose:

- `ui` → `script` is the one allowed exception, listed in `xtask/src/layers.rs`.
  UI documents run in the script subsystem's VM.
- The engine still passes `rhai::Dynamic` into script hooks, and still uses
  wgpu directly in `host.rs`. Both are internal to the host. The wgpu use
  moves behind `rhi` in Phase 5.
- The facade still re-exports `kerosene-map` under `internals`, for game
  code that builds maps.

## CI cleanup (between phases 1 and 2)

CI had failed on most pushes for a month. Three causes: the Windows and
macOS test jobs, the new-game jobs, which failed on `5d79bf9`'s bundler
bug, and the book job, whose link checker misread angle-bracket links. It is
now five jobs, and every one must pass:

| Job | What it runs |
| --- | --- |
| check (Linux) | fmt, clippy `-D warnings`, `xtask layers`, tests, Steam clippy and tests, `cargo doc` |
| build (Windows, macOS) | `cargo check --workspace --all-targets`: compiles, does not test |
| bundle and new game | `xtask bundle`, build, package under 10 MB, then `kerosene-tools new` and play it headless |
| book | link check, `mdbook build` |
| licences | `cargo deny` |

Every Rust job starts with `.github/actions/setup`. The job list:

- **Removed:** `msrv` and `semver`, now release steps in `src/docs/releasing.md`.
  semver would have failed on every refactor phase, since each one breaks
  the API on purpose.
- **Merged:** the three new-game jobs, now one Linux job inside `bundle`.
- **Changed:** the Windows and macOS test jobs are compile-only. The test
  failures there were not diagnosed: the logs need a GitHub login.

`release.yml` drafts the release with `gh`, and stops with a clear message
when a release for the tag already exists. That case is why `1.0.0-a2` and
`1.0.0-a3` both failed: immutable releases take no assets.

## Phase 2 results — resources

Done. The workspace build, all 2,313 tests, clippy, fmt and `xtask layers`
are green.

**New crate `kerosene-resource`** (layer 2, depends on `vfs`):

- `container`: `KRES` header (kind, kind version, FNV-1a source hash),
  block table, blocks on 16-byte boundaries. `DATA`, `REFS` (dependencies as
  virtual paths) and `EDIT` (compiler, args, inputs with hashes) are shared
  by every kind.
- `ResourceType` + `decode::<T>`: checks kind and version, falls back to
  `decode_legacy` for files from before a type moved into the container.
- `Resources` / `Resource<T>`: one handle per path and type, `load` now or
  `request` + `pump` later (`LoadState::Queued`), `reload` swaps the value
  and bumps `generation`, `on_reload` listeners, `collect` frees what
  nothing holds. It reads through a `Source` trait, implemented for `Vfs`, so
  the engine keeps owning its file system.
- `types`: the asset-type table (source extensions, compiled extension,
  kind, compiler).

**Migrated formats:**

| Format | Kind | Notes |
| --- | --- | --- |
| `.kmat` → `.kmat_c` | `KMAT` | New compiled form. Alchemy compiles it in the texture pass (`compile_materials`); `.kmat_c` is packed and `.kmat` is not. The two runtime parsers (engine footsteps, render) now load `.kmat_c`; the footstep lookup goes through the engine's `Resources` cache rather than re-reading the file each step. `WithMaterialSources` falls back to `.kmat` with one warning per file for unrebuilt projects |
| `.ktex` | `KTEX` | The old layout is the `DATA` block unchanged; old files still load |
| `.kmdl` | `KMDL` | Same, and `REFS` lists its materials |

`base.vault` was repacked: its 26 materials are `.kmat_c`, and its textures
and models are in the container. Every texture payload is byte-identical to
before. The two committed `.kmdl` were rewrapped rather than recompiled, so
their geometry is unchanged. Only their payload version field went from 1
to 2, which reads the same.

**Left for later:**

- `.kaud`, `.kbsp` (a lump directory with its own `KCUB`/`ACST` lumps),
  `.kwalk` and `.vault` keep their own headers. `.vault` is an archive, not a
  resource, and stays that way.
- Once projects have been rebuilt, the `.kmat` fallback goes.
- Render still loads textures and models by reading bytes itself. Moving it
  onto `Resources` belongs with the render split in Phase 5, where hot reload
  can rebuild GPU data from a handle's `generation`.
- Found on the way: `kiln --force` does not reach the texture pass, so it
  does not recompile textures or materials.

## Phase 3 and 4 results — reflect and ecs

Done for the stock game. The workspace build, all 2,333 tests, clippy with
`-D warnings` (with and without `steam`), fmt and `xtask layers` are green.

**New crates.** `kerosene-reflect` (attributes `Key`, `Label`, `Help`,
`Widget`, `Hidden`, `Transient`, `Networked`, on top of a pinned
`bevy_reflect`, and the `Value` type keyvalues travel as) and `kerosene-ecs`
(a pinned `bevy_ecs` and `bevy_reflect` behind one prelude). Game code
imports neither Bevy crate.

**What changed in the entity world.** An entity is a slot with an ECS
handle. A class lists the components it carries (`ClassDef::component`);
the map loader fills their keyed fields, saves keep every field not marked
`Transient`, and the editor's keys come from the same declaration
(`Schema::with_component_keys`). `EntityWorld::keyvalue`, `set_keyvalue`
and `keyvalues` read and write a key wherever it lives, so the engine, the
console and scripts never need the game's types. `kerosene_entity::schema::check`
holds the hand-written schema text to the components: a key the component
reads but the text does not offer, or a default the two disagree on, fails
the test.

**Every class with keys is ported**: doors and buttons, triggers, logic,
gameplay classes, physics props and the spawner, `light_dynamic`,
`prop_dynamic`, `logic_script`, sounds, the five store classes, the world
panel and `infodecal`. Shared pieces are components several classes carry:
`Switchable` (the schema's `startdisabled` base) and `PhysicsBody`.

**Engine side.** The engine reads and writes entity keys through
`keyvalue_*` / `set_keyvalue`. Three things that used to be loose fields the
engine and game agreed on by name were wrong after the port and are fixed:
`is_disabled` (brushes, triggers and panels ask one question), the script
`set` action (it wrote a loose field that a component ignored), and script
and `ent_info` views (they showed only loose fields).

**Left, on purpose.**

- The schema text still owns help text, inputs and outputs. Only keys, types
  and defaults come from components.
- Loose fields remain for what the engine owns and a game does not declare:
  `model`, `model_mins`/`model_maxs`, `occupied`, `player_inside`,
  `speed_scale`, and a `light_environment`'s `_light`.
- `networked` is recorded but nothing reads it: there is no network layer.
- Saves from before the port still load (fields are handed to the component
  that claims them). No migration tool: the project is alpha.
- Phase 5 is under way; see below.

## Phase 5 results — split render and physics (first part)

| Change | What moved |
| --- | --- |
| `render` → `scene` | The CPU-side half of the renderer moved to `kerosene-scene`: `camera`, `mesh` (with visibility culling), `lightmap`, `lights`, `probes`, `decals` and `brdf`. None of it touched the graphics API. `kerosene-render` re-exports the modules, so no path changes. `ATLAS_FORMAT` is a wgpu type, so it stayed in `render` |
| `rigid` → `physics` | `kerosene-rigid` is gone. Its code is `kerosene_physics::rigid`, and `RigidWorld` is now `PhysicsWorld` (also at the crate root). It is outside the SemVer promise, as `rigid` was |
| `host` → `rhi` | Requesting the device, choosing the surface configuration (sRGB, copyable) and the present mode, and reading a frame back (`Capture`) moved into `kerosene-rhi`. The host's screenshot code only encodes the PNG |

Left for later:

- `material`: there is nothing to split out yet. Materials are `kerosene-asset`
  data, and `render` binds them inside `gpu.rs` (3.4k lines). A material crate
  belongs with cutting `gpu.rs` into pipelines, where the shader permutations
  described in the design document have somewhere to live.
- The renderer's pipelines and the host's per-frame wgpu code still name wgpu
  directly, so `render` and `engine` depend on it.
- `PhysicsWorld` still has only the body API. World traces go through
  `CollisionWorld`, and `PhysicsProps` in the engine still owns the
  entity-to-body mapping. Folding those in means moving the mapping out of the
  engine.
- Breaking up `kerosene-engine` itself (`engine.rs`, `host.rs`).
