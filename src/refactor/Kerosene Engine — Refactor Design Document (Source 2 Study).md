# Kerosene Engine — Refactor Design Document (Source 2 Study)

Sep 28, 2026 · @T342, or T3

## Purpose and ground rules

Kerosene should be refactored toward Source 2's *architecture* — layered libraries, a single resource pipeline, reflected entities — while keeping its own brush/CSG world model, which Source 2 deliberately abandoned. This document studies Source 2 system by system, then turns each finding into a decision for Kerosene.

How to use it:

1. Sections on Source 2 end with a **Kerosene takeaway** — the one decision that section drives.
2. The target architecture section is the refactor's north star: every module in the bootstrap code gets mapped onto it.
3. The roadmap sequences the work so the engine stays runnable after every phase.

Clean-room rules for this study (carried over from the Source 1 reference work):

- Only public material: the Valve Developer Community wiki, Valve's shipped tools and their documentation, s&box's public docs and open-sourced code, GDC/SIGGRAPH talks, and published file-format write-ups.
- No leaked Valve source code and no decompiled Valve binaries — not as reference, not "just to check." Anything learned from them contaminates the implementation.
- Kerosene copies *ideas and published formats*, never code, shader source or asset content. File-format compatibility (reading `.vmdl_c` etc.) is a separate, later legal question.

Open question: this study has not yet seen Kerosene's code. The audit checklist in the roadmap is written so the bootstrap can be scored against it; sharing the repo layout (or a file tree) would let the mapping be made concrete.

## Source 2 at a glance

Source 2 is a C++ engine planned from 2007, announced at GDC 2015, and shipped first as the Dota 2 port in September 2015 ([Wikipedia](https://en.wikipedia.org/wiki/Source_2)). Valve's stated goal was faster content creation, plus Vulkan support and an in-house physics engine (Rubikon) replacing Havok. The lesson for Kerosene: Source 2 was an *evolution* of Source 1's code base, rebuilt around tools and data pipelines — not a rewrite from zero.

| Year | Title | What it proved |
| --- | --- | --- |
| 2026 | s&box (Facepunch) | Third-party game platform on Source 2, C# scene/component layer |
| 2024– | Deadlock (beta) | Current flagship; networked hero shooter |
| 2023 | Counter-Strike 2 | Competitive networking, CS:GO content migration |
| 2022 | Aperture Desk Job | Steam Deck tech demo |
| 2020 | Half-Life: Alyx | Single-player VR, heavy physics interaction, public Hammer/ModelDoc tools (May 2020) |
| 2020 | Dota Underlords | Mobile (Android/iOS) targets |
| 2018 | Artifact | Card game; first mobile push |
| 2016 | Robot Repair (The Lab) | VR |
| 2015 | Dota 2 Reborn | First shipped Source 2 game, ported from Source 1 |
| 2010 | Left 4 Dead 2 remake demo | First internal tech demo |

### What changed from Source 1

The table is a summary; each row is expanded in its own section below.

| System | Source 1 | Source 2 |
| --- | --- | --- |
| World geometry | Convex brushes → BSP compile (vbsp/vvis/vrad) | Editable polygon meshes in Hammer; world compiled to meshes + visibility data |
| Assets | Loose source formats (.vmt/.vtf/.mdl) | Source files (.vmat, .vmdl…) compiled by resourcecompiler to `_c` binaries |
| Physics | VPhysics wrapper over Havok | Rubikon, in-house |
| Rendering | DirectX 9 era, shader combos | Vulkan/DX11 abstraction, VFX shader files |
| Scripting | VScript (Squirrel in later branches) | Lua in Dota/Alyx; C# in s\&box; game-specific |
| UI | VGUI / Scaleform | Panorama (layout/XML + CSS-like styles + JS) |
| Entities | C++ classes, datadesc/netprop tables, I/O | Same I/O model; reflection via a schema system |

Kerosene takeaway: copy Source 2's *pipeline discipline* (source → compiled asset, schema-driven entities) while keeping Source 1's *world model* (brushes), which is the identity of the project.

## Core architecture and module layout

Source 2 keeps Source 1's strict layering: a platform library at the bottom, shared services above it, one module per subsystem, the engine, then a client/server pair per game. The module names below are the DLLs visible in a Counter-Strike 2 install ([CS2 SDK module list](https://www.cs2-sdk.com/)).

&#91;embedded content: Source 2 module layers · as shipped in Counter-Strike 2\]

Arrows point from a layer to what it may call; nothing calls upward, and the tools reuse the runtime rather than reimplementing it.

Three patterns hold the layers together:

- **App systems.** Every subsystem is an object with a fixed lifecycle (connect to dependencies → init → shutdown → disconnect), loaded by the host in dependency order. This pattern is documented in Valve's public Source SDK 2013 `appframework`, and Source 2 still ships one module per system.
- **Interfaces by name.** Modules expose versioned interfaces (e.g. `SchemaSystem_001`) through a factory export, so any layer can be swapped — Source 1 did this to swap DX9 for an empty renderer on dedicated servers ([Source.NET notes](https://github.com/thevurv/Source.NET)).
- **Client and server as separate game modules.** Both are compiled from shared game code, and the engine can host both in one process for listen servers. The server is authoritative; the client predicts and interpolates.

Kerosene takeaway: define the layers and lifecycle *before* moving code. A bootstrap written by an agent tends to let any file call any other; the refactor's first rule is that each module lists its allowed dependencies and nothing reaches upward.

## Resource system and asset pipeline

Every Source 2 asset exists twice: an editable source file in `content/` and a compiled binary in `game/` whose extension ends in `_c` (e.g. `.vmdl` → `.vmdl_c`) ([Source 2 Viewer / VRF](https://github.com/ValveResourceFormat/ValveResourceFormat)). The runtime only ever loads compiled resources. This single rule is what makes Source 2's tools, hot reload and packaging work.

The pipeline, in order:

1. **Author** a source file. Most are KeyValues3 (KV3) — a typed, JSON-like text format with a version header — or a raw input (PNG/TGA, FBX/DMX, WAV).
2. **Look up the asset type.** A KV3 config in `game/bin` (`assettypes_common.txt` in CS2, `sdkassettypes.txt` in the SDK) maps each extension to a compiler routine such as `CompileTexture`, and lists which other files it depends on ([VDC: assettypes\_common.txt](https://developer.valvesoftware.com/wiki/Assettypes_common.txt)).
3. **Compile** with `resourcecompiler`. Dependencies compile first (a material pulls in its textures; a model pulls in its materials).
4. **Write a resource file**: a header plus a list of typed blocks — external references to other resources, edit info recording the inputs and compile arguments, and the payload data.
5. **Package** compiled files into VPK archives for shipping; loose `_c` files override during development.
6. **Load at runtime** through `resourcesystem`: game code asks for a resource by path and gets a handle; loading can happen asynchronously, and references are tracked so unused resources can be freed.

What each piece buys Source 2:

| Mechanism | Payoff |
| --- | --- |
| Source/compiled split | Runtime code never parses slow authoring formats; formats can change without breaking tools |
| Edit info block (inputs + args) | Tools know when a compiled file is stale and recompile it on demand |
| External reference block | The dependency graph is data, so packaging and preloading are automatic |
| Handles instead of raw pointers | Hot reload swaps the data behind a handle while the game keeps running |
| One asset-type registry | Adding a new asset type is a config entry plus a compiler, not engine surgery |

Note on sources: Valve publishes no format specification; everything past the VDC page comes from reverse-engineering work such as Source 2 Viewer. Kerosene should borrow the *structure*, not the byte layouts.

Kerosene takeaway: build a `resource` module early — an asset-type registry, a compiler per type, a common compiled container (header + typed blocks + dependency list + source hash), and handle-based loading. Brush maps then become just one more asset type the compiler understands.

## World representation and level editing

Source 2 dropped BSP brushes entirely: Hammer edits polygon meshes, and `func_detail` and hint brushes no longer exist ([Steam forum, SDK discussion](https://steamcommunity.com/app/211/discussions/0/4353369456413071982/)). This is the one place Kerosene should deliberately *not* follow Source 2 — but it should adopt the compile back end that replaced the BSP tools.

### How a Source 2 map compiles

A CS2 build log shows the stages: an incremental-build check, "Building 'world'", map visibility built by voxelizing the level (8-unit voxels in that log), then `vrad3` baking lighting on the GPU through Vulkan ray tracing ([CS2 compile log](https://steamcommunity.com/app/211/discussions/0/4331979346097745353)).

1. **Mesh merge.** Static geometry and props are merged into world batches unless a mesh opts out; a prop can be baked into the world so the model is not referenced at runtime ([VDC: prop\_static](<https://developer.valvesoftware.com/wiki/Prop_static_(Source_2)>)).
2. **Visibility.** The compiler uses an inside/outside test to decide which space is "inside" the map, then builds clusters. There are no BSP leaks, but visibility still spills through holes in the geometry ([Source2 Wiki: Visibility](http://www.source2.wiki/EngineTools/HammerEditor/visibility)). Every mesh contributes unless marked "Exclude from VIS".
3. **Vis tuning.** `visibility_hint` volumes set a coarser or finer voxel grid per region, and the solver may merge across splits to hit a target cluster count ([VDC: visibility\_hint](https://developer.valvesoftware.com/wiki/Visibility_hint)).
4. **Lighting.** Each mesh chooses lightmaps or light probes; probed meshes skip merging unless baked to world ([VDC: prop\_static](<https://developer.valvesoftware.com/wiki/Prop_static_(Source_2)>)).

### Brushes vs meshes vs Kerosene

| Concern | Source 1 (brushes → BSP) | Source 2 (meshes) | Kerosene proposal |
| --- | --- | --- | --- |
| Authoring primitive | Convex additive brushes | Free-form polygon meshes | Convex brushes, subtracted from solid space |
| "Inside" of the map | Flood-fill from entities; leaks break the compile | Voxel inside/outside test; holes leak vis | Exact: inside = union of subtracted brushes, so leaks are impossible by construction |
| Visibility | Portals → PVS (vvis) | Voxel clusters, hint volumes | Start with portals from brush adjacency; add voxel clusters later if needed |
| Collision | Brushes are the hulls | Separate collision meshes | Derive from the CSG result; brushes give clean convex pieces |
| Detail geometry | `func_detail` | Everything is a mesh | Props and non-vis meshes, as Source 2 |
| Lighting | vrad lightmaps | Lightmaps or probes per mesh, GPU bake | Lightmaps first (brush faces UV well), probes for props |
| Iteration | Full recompile | Incremental build | Incremental: rebuild only brushes whose bounds changed |

The key insight: subtractive CSG (the Unreal Engine 1 model) gives Kerosene something Source 2 has to approximate with voxels — an exact definition of playable space. That makes visibility, leak checks and navigation seeding cheaper and more reliable.

Kerosene takeaway: split the map into a **source** format (brushes, entities, props) and a **compiled** world resource (merged meshes, collision, vis clusters, lightmaps), produced by a staged, incremental compiler. The brush CSG kernel is one stage of that compiler, not something the runtime ever touches.

## Entity system

Source 2 keeps Source 1's entity model — C++ classes, keyvalues from the map, inputs and outputs — but describes every class through one reflection system, the schema. In Source 1, only *networked* fields exposed their layout; in Source 2 the schema records fields, base classes, static members, nested enums and per-field metadata for most classes, networked or not ([praydog, 2015](https://praydog.com/reverse-engineering/2015/06/24/source2.html)).

What the schema carries, per field, is the important part. Metadata tags seen in public dumps include `MNetworkEnable` / `MNetworkDisable` (replicate or not), `MNetworkPriority`, and `MKeyfieldname` (the name a map keyvalue binds to) ([praydog, 2015](https://praydog.com/reverse-engineering/2015/06/24/source2.html)). One declaration therefore drives several systems:

| Consumer | What it reads from the schema |
| --- | --- |
| Map loader | Keyvalue name → field, type, default |
| Networking | Which fields replicate, priority, encoding |
| Save/restore | Every field and its type |
| Editor (Hammer entity properties) | Field names, types, help text |
| Debugging | Live dump of any class layout (`schema_dump_binding`-style console commands) |
| Script bindings | Callable fields and types |

Other parts of the model to carry over:

- **Identity split from behaviour.** Entities have a separate identity record (name, handle, flags) from the instance holding game logic, so handles stay valid and lookups by name are cheap.
- **Components on entities.** Classes such as a script component hang off entities rather than living in a deep inheritance chain; s&box takes this furthest with a GameObject + Component scene model.
- **Entity I/O.** Hammer's output → target → input connections with delay and parameter survive unchanged from Source 1. It is the level designer's scripting layer and costs little to implement once the schema can name inputs.
- **Per-module type scopes.** Client and server each register types into their own scope, so the same class name can differ between them.

Clean-room note: these details come from debug symbols Valve shipped in 2015 Mac builds. Use them as evidence of the *design*, not as a layout to copy.

Kerosene takeaway: introduce a reflection layer before rewriting entities. Each entity field is declared once with metadata (`keyvalue`, `networked`, `saved`, `editor`), and the map loader, network layer, save system and editor all read that declaration. In Rust this is a derive macro on each component struct. Bootstrap code that hand-parses keyvalues per class is the first thing to delete.

## Entities in Rust: an ECS

Kerosene should store entities in an entity-component-system (ECS), mainly because it suits Rust's ownership rules — but Godot is not the model to follow here. Godot is explicitly not ECS-based: it composes higher-level nodes in a tree ([Godot: Why isn't Godot an ECS-based engine?](https://godotengine.org/article/why-isnt-godot-ecs-based-game-engine/)). The short IDs you have seen in Godot are its `uid://` resource UIDs, which keep references between *files* intact when they are renamed or moved ([Godot docs: ResourceUID](https://docs.godotengine.org/en/latest/classes/class_resourceuid.html)). Kerosene needs that idea too, but in the resource system, separate from entity identity.

### Four kinds of identity

| ID | Names | Lives for | Kerosene form |
| --- | --- | --- | --- |
| Runtime entity | A live entity in a World | One session | Generational index: slot + generation, as `bevy_ecs` does ([PR #6740](https://github.com/bevyengine/bevy/pull/6740)). A stale handle fails the generation check instead of hitting whatever reused the slot — the same job Source's entity handles (index + serial) do |
| Map entity | An entity placed in the editor | As long as the map file | Random 64-bit ID written by the editor into the map source; saves, net spawns and compiler output refer to it; mapped to a runtime entity at load |
| Asset | A file | Survives renames and moves | Godot-style UID: random 64-bit value stored beside the file, resolved by `resource`; shown as a short base-36 string |
| Name | `targetname` for entity I/O | Designer's choice | `Name` component plus a name → entities index; not unique, so one output can hit many targets |

Avoid using a hash of a name as identity: names change and hashes can collide. Short hash-like strings are fine as a *display* form of a random ID.

### Why ECS fits Rust

- **No pointer graphs.** Entities holding references to each other fight the borrow checker (`Rc<RefCell<…>>` everywhere). In an ECS the World owns all data and entities refer to each other by ID.
- **Declared access.** Each system states which components it reads and writes, so the scheduler can run non-conflicting systems in parallel without data races.
- **Composition instead of inheritance.** Source's deep class chain (base entity → animating → prop → door) becomes components added to an entity.
- **Caveat.** ECS does not make code reliable on its own. Reliability comes from generational IDs, declared access and tests; ECS makes those cheap.

### Crate choice

| Option | What you get | Cost |
| --- | --- | --- |
| `bevy_ecs` (recommended) | Built for Bevy but usable as a standalone crate ([crates.io](https://crates.io/crates/bevy_ecs)); queries, schedules, change detection, events/observers, multiple storage types; `bevy_reflect` can back the reflection layer | Breaking API changes most releases — pin the version and upgrade deliberately |
| `hecs` | Minimal, fast archetype storage; "a library, not a framework" ([docs.rs](https://docs.rs/crate/bevy_hecs/0.1.0)) | You write the scheduler, events and change detection yourself |
| Your own | Full control | Months of work that is not Kerosene's identity |

Wrap whichever you pick in a thin `ecs` facade module, so game code imports Kerosene types and a crate upgrade touches one place.

### Source concepts, ECS form

| Source 1/2 concept | Kerosene ECS form |
| --- | --- |
| Entity class (e.g. a rotating door) | A spawn recipe: the component set built from the class definition in the entity-definition file |
| Keyvalues | Component fields tagged `keyvalue` through `reflect`; the map loader fills them |
| Think functions / next-think time | Systems; per-entity timers as a `NextThink` component checked by one scheduler system |
| Entity handles | Generational `Entity` IDs |
| Inputs and outputs | An `Outputs` component holds connections; firing pushes events into an I/O queue with delays; a dispatch system delivers due events to input handlers registered per component |
| Networked variables | Fields tagged `networked`; a snapshot system diffs them each tick |
| Client and server | Two separate Worlds in one process, never sharing data |
| Parenting | Parent/children components plus a transform-propagation system |
| Drawables | An extract system copies render data into `scene`; the renderer never queries the game World |

### Server tick order

1. Apply client inputs, in timestamp order.
2. Dispatch due entity I/O events, ordered by fire time then sequence number, so a map behaves identically every run.
3. Gameplay systems: think, AI, weapons, triggers.
4. Step physics through `PhysicsWorld`, then write transforms back.
5. Propagate parent transforms.
6. Build the network snapshot from `networked` fields.

The client runs receive snapshot → predict → interpolate → extract to scene → render.

Kerosene takeaway: add `ecs` to the core-services layer beside `reflect`. Game code becomes components and systems, entity classes become data-driven spawn recipes, and entity I/O becomes an ordered event queue.

## Rendering

Source 2 renders in three layers that never skip each other: a graphics-API backend (`rendersystemdx11`, Vulkan), a material system that binds shaders to parameters, and a scene system that owns everything drawable. Game entities never issue draw calls; they own *scene objects* that the scene system culls and batches.

| Layer | Source 2 module | Responsibility |
| --- | --- | --- |
| Render hardware interface | `rendersystemdx11` / Vulkan backend | Buffers, textures, pipelines, command lists; one backend loaded per run |
| Materials | `materialsystem2` | Material = shader + parameter values + textures, loaded as a compiled resource |
| Scene | `scenesystem` | Scene worlds, scene objects, views, culling, batching, lights |
| Effects | `particles` | Particle systems as data assets rendered through the scene |

### Shaders: the VFX format

Source 2 shaders are single text files wrapping HLSL in named blocks ([s&box wiki: Anatomy of shader files](https://wiki.facepunch.com/sbox/AnatomyOfVFX)):

- `HEADER` — description and compile targets.
- `MODES` — which render passes the shader supports (e.g. forward, depth-only, shadow).
- `FEATURES` — declared permutation axes, e.g. `Feature(F_ENABLE_DEPTH, 0..1, "Settings")`, which the material editor exposes as toggles.
- `COMMON` — HLSL shared by all stages; `VS`, `PS`, plus `GS` and `CS` — the stage programs.

The compiler builds each needed combination of features × modes ahead of time into a compiled shader resource, so the runtime never compiles shaders. s&box adds node-based Shader Graph and shader-function assets on top of the same format ([VDC: asset types](https://developer.valvesoftware.com/wiki/List_of_Source_2_asset_types)).

### Lighting and visibility at runtime

- Baked lightmaps or light probes per mesh, chosen at compile time (see the world section).
- Reflection via cubemaps and combined light-probe volumes placed as entities.
- Precomputed vis clusters plus GPU culling of merged world aggregates — CS2 exposes toggles for both ([Source2 Wiki: Visibility](http://www.source2.wiki/EngineTools/HammerEditor/visibility)).

Kerosene takeaway: separate `rhi` (thin API wrapper, one backend — Vulkan or a portable layer such as wgpu), `material` (compiled shader + parameters) and `scene` (drawables, views, culling). Adopt the VFX idea of *declared* features and passes per shader, compiled offline, instead of ad-hoc `#define` strings built at runtime.

## Physics, animation, audio, networking, scripting

Each of these is its own module in Source 2, reached through an interface, and each consumes compiled resources. Kerosene does not need Valve-scale versions of any of them; it needs the same *boundaries* so each can start simple and be replaced later.

| System | Source 2 approach | Kerosene decision |
| --- | --- | --- |
| Physics | Rubikon, in-house since 2012, replacing Havok; module `vphysics2` ([UploadVR](https://www.uploadvr.com/half-life-alyx-rubikon-physics/)) | Wrap an existing library (Jolt, Rapier, or PhysX) behind a `physics` interface; never let game code touch the library directly |
| Collision data | Separate physics hulls compiled into model and world resources | Compile hulls offline; world collision comes from the CSG result |
| Animation | `animationsystem` driven by animation-graph assets (AnimGraph, sub-graphs) | Start with clip playback + blending; keep the graph as a data asset so tools can grow into it |
| Audio | `soundsystem`; game code triggers named sound events defined in data, not raw files | Adopt sound events from day one: code says `play("door.open")`, data picks files, volume, pitch, falloff |
| Networking | `networksystem`; server-authoritative snapshots of schema-marked fields. CS2 keeps 64 Hz ticks but timestamps inputs within a tick ("sub-tick") ([AFK Gaming](https://afkgaming.com/csgo/guide/what-is-sub-tick-in-counter-strike-2)) | Design for client/server even in single-player: a local server + client in one process, as Source does. Replicate only fields the reflection layer marks `networked` |
| Level logic | Entity I/O in Hammer; `pulse_system` graphs in newer titles | Entity I/O first (cheap, proven); graphs are optional later |
| Game scripting | Game-specific: Lua in Dota 2 and Half-Life: Alyx, C# in s&box, native C++ in CS2 | Pick one embedded language only after the reflection layer exists; bindings should be generated from it, not hand-written |
| UI | `panorama`: layout files + CSS-like styles + script | Use an existing immediate-mode UI for tools and debug; defer game UI |

Kerosene takeaway: for every system above, write the interface first and the simplest implementation second. The bootstrap probably called libraries directly from game code; those call sites are where the interfaces go.

## Target architecture for Kerosene

Kerosene should adopt Source 2's layer stack almost unchanged, with one structural difference: the brush/CSG code lives in a tools-only library, and the runtime loads only compiled worlds.

&#91;embedded content: Kerosene target module map · 7 layers + tools\]

Each arrow points down to what a layer may use; the highlighted `csg` library is the one piece Source 2 has no equivalent for.

### Dependency rules

1. A module may depend only on modules in lower layers. No upward calls, no cycles — enforce it in the build (separate crates or libraries), not by convention.
2. Subsystems never depend on each other directly. When render needs physics debug shapes, the engine passes data between them.
3. Game code talks to subsystems only through their interfaces, never through the underlying library (no raw physics or graphics-API types in game code).
4. Only `kcompile` and `editor` link `csg`. If the runtime needs brush math, that is a sign something should be compiled instead.
5. Every asset the runtime loads comes through `resource` as a compiled file; runtime code never parses authoring formats.
6. Every entity field the map, network, save system or editor needs is declared once through `reflect`.

### Interfaces to define first

| Interface | Minimum surface | Consumers |
| --- | --- | --- |
| `System` lifecycle | `init`, `shutdown`, declared dependencies | host |
| `Resource<T>` handle | load by path, async state, reload callback | everything above core |
| `TypeInfo` | fields, types, metadata flags | map loader, net, saves, editor |
| `Rhi` | device, buffers, textures, pipelines, submit | material, scene |
| `PhysicsWorld` | bodies, shapes, queries (trace, overlap), step | game-shared, world |
| `Entity` | spawn recipes from keyvalues, queries, systems, ordered I/O event queue (over the ecs facade) | game-server |

Decided: Kerosene is Rust, and the implementation language decides how `reflect` is built: a derive macro, or bevy\_reflect with Kerosene's own attributes. Each module above becomes its own crate in one Cargo workspace, so the compiler itself enforces rule 1.

## Refactor roadmap

Refactor in six phases, bottom layer first, and keep the engine running after every phase — no big-bang rewrite. Foundation and resources come before the world compiler because the compiler is just the most complex user of both.

&#91;embedded content: Refactor roadmap · 6 phases, 6 gates, not to scale\]

A phase is done only when its gate passes; work that belongs to a later phase gets a stub interface now and an issue for later.

### Phase 0 audit checklist

Score the bootstrap against these before moving any code:

- [ ] Each source file is tagged with its target module from the architecture map (or marked for deletion)
- [ ] Every dependency that points upward or sideways between subsystems is listed
- [ ] Every place runtime code parses an authoring format (map source, images, model source) is listed
- [ ] Every place game code calls a third-party library directly (graphics API, physics, audio) is listed
- [ ] Every entity class that hand-parses its own keyvalues is listed
- [ ] Brush/CSG code reachable from the runtime binary is identified
- [ ] Global mutable state and singletons are listed with their owners
- [ ] Features that are half-implemented stubs are marked keep, finish or cut
- [ ] CI builds every target and runs a boot-and-load-map smoke test

### What to keep from the bootstrap

- Working math, containers and platform code: move into `core` unchanged if tests pass.
- The CSG kernel, if it produces correct results: move into `csg` and wrap it in compiler stages rather than rewriting it.
- Any renderer that draws correctly: keep it running behind the new `rhi` boundary, then split material and scene out of it.
- Rewrite, rather than move: keyvalue parsing per entity, ad-hoc asset loading, and anything that mixes editor and runtime code.

### Risks

| Risk | Mitigation |
| --- | --- |
| Refactor stalls with the engine broken | Gates require a booting build; never start a phase with the last one red |
| Reflection layer grows into a framework | Ship only the metadata flags the table in the entity section needs |
| Agent-written code hides subtle bugs | Add tests before moving a module, not after |
| Chasing Source 2 feature parity | Each phase delivers one gate; Rubikon-, Panorama- or AnimGraph-scale work is out of scope |
| Clean-room contamination | Keep the ground rules at the top of this doc; reference only public material |

## Sources

Pages opened for this study, all public. Items marked (RE) are community reverse-engineering: fine as evidence of design, not as layouts to copy.

| Source | Used for |
| --- | --- |
| [Wikipedia: Source 2](https://en.wikipedia.org/wiki/Source_2) | History, titles, Vulkan and Rubikon goals |
| [CS2 SDK module list](https://www.cs2-sdk.com/) (RE) | Shipped module names |
| [Source.NET](https://github.com/thevurv/Source.NET) | Tiered library and interface pattern notes |
| [Source 2 Viewer / ValveResourceFormat](https://github.com/ValveResourceFormat/ValveResourceFormat) (RE) | Compiled `_c` resource files, block structure |
| [VDC: assettypes\_common.txt](https://developer.valvesoftware.com/wiki/Assettypes_common.txt) | Asset-type registry, compiler routines |
| [VDC: List of Source 2 asset types](https://developer.valvesoftware.com/wiki/List_of_Source_2_asset_types) | Asset types, shader graph assets |
| [Steam forum: func\_detail is gone](https://steamcommunity.com/app/211/discussions/0/4353369456413071982/) | Removal of BSP brushes |
| [Steam forum: CS2 compile log](https://steamcommunity.com/app/211/discussions/0/4331979346097745353) | Map compile stages, voxel vis, vrad3 |
| [VDC: prop\_static (Source 2)](<https://developer.valvesoftware.com/wiki/Prop_static_(Source_2)>) | Mesh merging, bake to world, lighting type |
| [VDC: visibility\_hint](https://developer.valvesoftware.com/wiki/Visibility_hint) | Vis grid resolution, cluster merging |
| [Source2 Wiki: Visibility](http://www.source2.wiki/EngineTools/HammerEditor/visibility) | Inside/outside vis, leaks, GPU culling |
| [praydog: The Schema System](https://praydog.com/reverse-engineering/2015/06/24/source2.html) (RE) | Schema reflection, field metadata |
| [s&box wiki: Anatomy of shader files](https://wiki.facepunch.com/sbox/AnatomyOfVFX) | VFX shader block format |
| [UploadVR: Rubikon](https://www.uploadvr.com/half-life-alyx-rubikon-physics/) | Physics engine history |
| [AFK Gaming: CS2 sub-tick](https://afkgaming.com/csgo/guide/what-is-sub-tick-in-counter-strike-2) | Tick rate and timestamped input |

Further reading worth fetching next: Valve's public Source SDK 2013 (`appframework`, entity I/O) and the s&box open-source engine layer, both legitimately published.
