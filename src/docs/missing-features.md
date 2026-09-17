# Missing features

An inventory of what Kerosene does not have yet, in four parts: what the code
reveals that the docs did not admit, what the docs already admit, what is
absent by subsystem, and what the mainstream engines have that is worth
borrowing -- and what is not. Read it as a candidate roadmap, not a bug list:
several of these are deliberate design choices (a flat entity list, a closed
shader set) and several are simply unstarted. The last section puts an order
on it.

For which of these absences actually matter -- and which genres the engine is
shaped to serve -- see [`positioning.md`](positioning.md). Everything here is
weighed against that document's answer: a movement shooter or an immersive
sim first, geometry-heavy and animation-light.

## 1. What the code says that the docs did not

These were found by reading the tree rather than the README, and each is
either a correctness gap or a thing whose absence undercuts a claim made
elsewhere in these docs.

- ~~**The view is not interpolated between ticks.**~~ Fixed: the host
  reads `Engine::interpolation_alpha()` and takes the view angles from the
  latest input rather than the last tick.
- ~~**No continuous integration.**~~ Fixed: `.github/workflows/ci.yml` runs
  `fmt --check`, `clippy -D warnings` and the tests on Linux, and builds on
  Windows and macOS.
- ~~**No panic hook, no crash log.**~~ Fixed:
  `kerosene_console::install_crash_handler` writes `crash.log` beside the
  binary with the panic, a backtrace and the last 64 log lines.
- **No demo recording.** The most conspicuous Source feature not here, and
  the one the architecture most obviously supports: the tick is fixed, the
  simulation runs headless, and input arrives as an `InputState` per tick.
  Recording those is nearly free and buys replays, ghosts for the time-trial
  genre, bug reports that reproduce, and a regression harness for the
  movement solver. The determinism claim in `positioning.md` is unproven
  until something replays.
- **No game-state serialization at all.** There is no `serde` anywhere in the
  tree. Save/load, state that survives a map transition, and cloud saves all
  wait on this.
- **`volume` is the only sound convar an options screen could set.** There
  is no separate music or effects volume.
- **`kerosene-ui` is not a UI toolkit.** It is the window that hosts the
  tools -- winit, a wgpu surface and egui in three hundred lines. The name is
  spoken for by the thing a game actually needs (section 8), and freeing it
  is cheaper now than later.
- **Chisel is a quarter of the codebase in one crate.** Eighteen thousand
  lines; worth splitting before the GPU viewport lands.

## 2. Already acknowledged

These come straight from the README's known-limits section and are kept here
for completeness.

- **Networking.** The simulation runs headless (the hard part is done), but
  there is no client/server wire protocol, no snapshot, no prediction, and no
  replication.
- **Skeletal animation.** `.keromdl` carries bones and per-vertex weights and
  Forge preserves them, but nothing plays animation.
- **Chisel 3D view.** Software-rasterised, with correct occlusion but no
  lighting or shadow preview.
- **Texture block compression.** `.kerotex` is uncompressed; no BCn. The
  README's reason -- a bad encoder is worse than none -- no longer holds:
  `intel_tex_2` and `texpresso` are pure-Rust BC7 encoders that are good
  enough.
- ~~**Mipmaps at runtime.**~~ Fixed: the whole chain is uploaded.
- ~~**Texture dimensions in the BSP.**~~ Fixed: Cleave reads each material's
  compiled texture (from the project found next to the map, or `--content`)
  and writes its real size, warning by name for any it cannot find. Maps
  compiled before this carry the old 512 and want a recompile.
- **Audio.** Stereo only. Falloff and panning exist; occlusion, reverb, and
  doppler do not.

## 3. Rendering and visuals

- **Dynamic/real-time lighting and shadows.** Everything is baked by Radiance.
  No shadow mapping, no runtime point/spot lights, no moving light sources.
  `light`, `light_spot`, and `light_environment` are compile-time only. One
  unshadowed point light in the shader is enough for a flashlight and a
  muzzle flash, and is the version worth doing first.
- **Post-processing.** No bloom, SSAO, color grading, or motion blur; only a
  single `mat_exposure` convar.
- **Anti-aliasing.** `multisampled: false` in the GPU setup; no MSAA/TAA/FXAA.
  MSAA is a one-line toggle in wgpu and should be a convar.
- **PBR material model.** Materials are a small closed set
  (`lit`/`unlit`/`sky`/`water`/`ui`). They carry colour, normal, roughness,
  emissive and occlusion maps, and the renderer samples all five -- but the
  specular term is a Blinn-Phong lobe steered by roughness rather than a real
  microfacet BRDF, and there is no metalness map, no parallax, and no
  per-material shader customization. Because diffuse lighting is baked and
  direction-free, a world surface's highlight is a guess from the view angle
  rather than from any light that still exists at draw time. Cubemap probes
  baked by Radiance would make the existing materials read correctly without
  touching the BRDF.
- **Level of detail.** No LOD for models or geometry.
- **Decals / projected textures.** None. A bullet hole is the first thing a
  weapon needs.
- **Particles / VFX.** None. Muzzle flash, sparks, dust.
- **Reflections.** No cubemap probes, no SSR, no planar reflections.
- **Dynamic sky / weather / time-of-day.** Sky is a static skybox; sun and
  sky lighting are baked.
- **GPU instancing.** The world draws per material, but repeated `prop_static`
  meshes are not instanced.
- **Debug labels.** No wgpu labels or `push_debug_group` per pass, so a
  RenderDoc capture is a list of unnamed passes. Costs nothing to add.

## 4. Physics and simulation

- **Rigid-body dynamics.** `kerosene-physics` is the player movement solver
  only. `kerosene-rigid` wraps Box3D (via `box3d-rust`) and is wired into the
  engine: world brushes become static hulls, `func_detail` joins them, and
  moving brush entities (doors, shutters) become static bodies that follow
  their entity's pose, so a closed door blocks a thrown prop. `prop_physics`
  entities get dynamic box bodies (from their model bounds) whose **object
  properties** -- `mass`, `friction`, `elasticity`, `pickable` -- decide how
  heavy, grippy, bouncy and grabbable each one is, and a
  `prop_dynamic_spawner` drops them on demand (inheriting those properties).
  The player is in the simulation too, as a kinematic body, so props bounce
  off them rather than through them, and walking into one shoves it: how far
  is a contest between a fixed `phys_player_push_force` and the prop's own
  `mass`, so a crate slides and a safe does not. The use key is a pick-up
  tool: aim at a prop and press use to carry it, press use again to set it
  down, or attack to throw it at `phys_launch_speed` plus whatever the player
  was already doing. A carried prop is steered toward the hold point rather
  than placed at it -- driven by impulses under ceilings on speed,
  acceleration and turn (`phys_hold_speed`, `phys_hold_accel`,
  `phys_hold_spin`, `phys_hold_spin_accel`), braking early enough to arrive
  rather than overshoot -- so it stays an ordinary body in the simulation: it
  turns to face the player as they turn, shoves what it can move, and hangs
  back short of the hold point when it meets what it cannot, rather than
  being driven through it. `.keromdl` models render at their simulated pose,
  and `phys_debug` draws the collision boxes. What is not there:
  **convex-hull props** (see the callout below -- the solver does hulls, props
  do not use them), joints, per-surface-material friction, a launch-beams
  gravity gun, turning a prop while carrying it (Source's use-plus-mouselook),
  and any sound or effect when a prop hits something.
- **Ragdoll / skeletal physics.** None. Box3D has joints, but no skeleton
  attachment.
- **Vehicles / wheeled physics.** None. Box3D has wheel joints, but no vehicle
  controller.
- **Cloth / soft body / fluid simulation.** Water is a volume flag, not
  simulated. Props fall through water rather than floating.
- **Projectiles / ballistics.** No projectile physics. Traces exist for the
  player, but there is no weapon system to use them.
- **Generalized physics queries.** Traces exist, but there is no public
  sweep/overlap API exposed for gameplay beyond movement.
- **Determinism is asserted, not audited.** `rayon` is on the compile side
  today, but nothing guards the simulation path against it, or against a
  `HashMap` iteration order reaching gameplay. The demo replay in section 1
  is the test that would catch it.

## 5. Animation and characters

- **Skeletal animation playback.** Bones and weights are stored; nothing
  animates them. Viewmodels are the first consumer, and the only one the
  target genres need soon.
- **Animation blending / state machines** (the Animator equivalent). None.
- **Morph targets / blend shapes.** None.
- **Inverse kinematics** (foot placement, look-at). None.
- **Animation retargeting.** None.
- **Third-person character.** Only a first-person controller, which is
  otherwise solid: walk/run/jump/air-strafe, duck, swim, ladders, noclip,
  step-up, health/fall-damage/respawn.

## 6. AI and navigation

- **NPC entities.** No `npc_*` classes; `tools/npcclip` exists as a material
  but nothing is an NPC.
- **Pathfinding.** [`NavGraph`](crate::nav) -- the consumer the walkmap was
  built for -- builds a connectivity graph of walkable faces sharing edges and
  A*-searches it for a list of waypoints. There is still no A* smoothing into
  a funnel, no flow fields, and no NPC entity to steer along the result; the
  query API exists and is tested, but nothing calls it yet.
- **Behavior trees / state machines / perception.** No AI decision-making and
  no sight or hearing queries.
- **Crowd / group movement.** None.
- **Dynamic navigation.** The static walkmap cannot reflect a door that is
  currently open or closed.

## 7. Audio

- **Occlusion / reverb / doppler** (acknowledged). A sound through a wall is
  as loud as one in the room. Occlusion is one BSP trace per voice; reverb
  is a soundscape entity or a per-leaf setting. Horror hinges on both.
- **3D spatialization.** No HRTF, no surround.
- **Audio effects / mixing buses.** The mixer is voices into a stereo buffer.
  No EQ, reverb sends, compression, ducking, or effects graph -- and no volume
  convars for an options screen to drive.
- **Streaming audio.** Sounds are decoded whole; no streaming for long
  ambience or music.
- **Footstep/impact effects.** `$surfaceprop` now parses into a
  [`SurfaceProperty`](kerosene_asset::SurfaceProperty), and the engine traces
  to resolve it and emits stride-timed footstep sounds (`footstep/<surface>/n`)
  named off it. Impact effects are still not emitted -- neither the player's
  own landings nor a physics prop striking a wall makes a sound, though the
  solver reports every one of those collisions -- and no footstep sound
  files ship yet, so a surface without assets warns once.
- **Procedural audio.** None.

## 8. UI and HUD

- **In-game HUD / menus.** The only in-game overlay is the developer console.
  No HUD, no main menu, no pause menu, no options screen, no dialogue boxes.
- **Gameplay UI toolkit.** No widget system, layout, or theming for shipped
  games. egui is used for tools and the console, not game UI, and the crate
  named `kerosene-ui` is the tool window, not this. Even a minimal
  immediate-mode layer of textured quads and an atlas font would unblock
  menus, options and a HUD at once.
- **Localization.** No string tables or translation. Cheap now, painful to
  retrofit.
- **Runtime text/font rendering** for gameplay. Only console and editor fonts.

## 9. Input

- **Gamepad / joystick support.** Keyboard and mouse only. `gilrs` is the
  pure-Rust answer; Steam Input replaces it on Steam and adds glyphs.
- **Input actions.** `bind` maps keys to commands, which is half of an action
  map; there is no layer that lets a gamepad and a keyboard both drive
  `+attack`, and no rebinding UI.
- **Touch / mobile input.** None.

## 10. Gameplay systems (beyond the FPS sandbox)

- **Weapons and combat.** No weapons, hitscan, ammo, or reload. Named as the
  gap three times in `positioning.md`; nothing is started. Hitscan through
  the existing traces is the first version.
- **Damage model.** Only player fall damage. No damage types, armor, enemy
  health, or hit reactions, and no `OnDamaged` output on entities.
- **Inventory / items / pickups.** None.
- **Objectives / quests / missions.** None.
- **Dialogue system.** None.
- **Save/load.** No game-state serialization; the console `.cfg` persists but
  gameplay state does not.
- **Persistent game flow.** Map transitions exist, but no state carries
  between them (health carries across respawn within a map, not across maps).
- **Difficulty / game settings.** None.
- **Entity classes.** The set is enough for the sample map and thin for a
  real one. Missing from Source's glue, roughly in the order a mapper hits
  them: `func_movelinear`, `func_tracktrain` and `path_track`, `logic_case`,
  `logic_compare`, `math_remap`, `filter_activator_name` and `_class`,
  `env_fade`, `env_shake`, `trigger_gravity`, `point_viewcontrol`,
  `env_sprite`, `game_ui`. None of these are hard; each is a class in
  `kerosene-game` and a row in the schema.

## 11. Content and asset pipeline

- **Prefabs / instances.** Brushes are authored per map; there is no way to
  include one `.keromap` in another. This is not a scene-graph feature --
  Hammer had instances too -- and past a handful of maps the brush workflow
  does not manage without it.
- **Scene graph.** Deliberately a flat entity list (a design choice, listed
  here for comparison with Unity/Unreal).
- **Material editor.** `.keromat` is hand-written KeyValues; no visual
  material graph.
- **Shader graph / custom shaders.** The shader set is closed.
- **Animation import in Forge.** Forge reads static OBJ only; no FBX/glTF, no
  skeletal or morph import.
- **Model LOD generation.** None.
- **Terrain tooling.** Brushes only; no heightmap terrain or terrain editor.
- **Level streaming / world partition.** One monolithic `.kerobsp` per map.
- **Visibility control for the mapper.** No `func_areaportal`, no hint or
  skip brushes, no occluders, so there is no way to steer Umbra on a level
  it gets wrong.
- **Runtime asset hot-reload.** Chisel reloads textures, but the running
  engine does not hot-reload materials, `.kerosnd`, scripts or the map.
  Chisel-to-engine iteration is F9, which is a full restart.
- **Project templates.** A new user has the sample map to start from and
  nothing else. `kerosene-tools new my_game` is a directory and a `.keroproj`.
- **Per-target build settings.** `.keroproj` names a content tree and a
  start map. A window title, an icon, default convars and a start map per
  configuration belong there too, along with named ship targets (Godot's
  export presets) for Kiln.

## 12. Networking and multiplayer

- **Client/server protocol** (acknowledged). No connection, snapshot, RPC, or
  entity replication.
- **Prediction and interpolation.** None between machines; see section 1 for
  the one that is missing on a single machine.
- **Dedicated-server story.** `--headless` exists, but there is no network
  stack to serve. If the engine goes Steam-first, Steam Datagram Relay is the
  pragmatic transport rather than a hand-written one.

## 13. Platform and distribution

- **Windows/macOS support.** wgpu, winit and cpal are cross-platform, so this
  is most likely untested rather than broken -- but nothing has tested it.
  CI is how it gets tested.
- **Mobile / console.** None.
- **Crash reporting.** A panic hook writes `crash.log` (section 1); there
  are no minidumps for native crashes and nothing that phones home. `sentry`
  covers both; Breakpad minidumps beside the log are the no-service option.
- **Installer / auto-updater.** None.
- **`build-content.sh` is a shell script.** Which is to say, Linux-only. A
  `cargo xtask` or a `just` file does the same on every platform.

## 14. Integrations

There are none. Which ones matter, in order of what they pay back:

- **Steamworks**, via the `steamworks` crate over Valve's SDK. The SDK is
  proprietary and cannot live in an LGPL/MPL tree, so this is a Cargo feature
  the engine is built with, and `kiln --ship --steam` copies
  `steam_api.so`/`.dll` and writes `steam_appid.txt`. Then, in order:
  - Init and `restart_app_if_necessary`. The overlay works over Vulkan and
    DX12 without help.
  - **Workshop.** Open formats plus a `.vault` per map means a map uploads
    from Chisel, a player subscribes in-game, and the engine mounts the
    download as one more VFS layer -- `kerosene-vfs` already does layered
    search paths. No other engine at this size makes this this easy, and it
    is the payoff of the open-formats thesis. See `positioning.md`.
  - Achievements and stats, exposed as an entity (`logic_achievement`) and a
    Rhai call, so a level can award one without code.
  - Cloud saves, once save/load exists.
  - Steam Input, for the gamepad story on Steam.
  - Rich presence; later, lobbies and Steam Datagram Relay for networking.
- **A platform trait** -- achievements, cloud, UGC, presence -- from the
  start, so Steam is one implementation and GOG Galaxy or Epic Online
  Services are another, and no game code names either.
- **Discord** rich presence (`discord-rich-presence`). Trivial.
- **Crash reporting** (section 13).
- **itch.io** as a Kiln ship target, via butler.

## 15. Kerosene-specific tooling gaps (editor and compilers)

Chisel:

- **3D lighting preview** (acknowledged). Baked lighting is not visible
  until compile and run.
- **VisGroups.** Nothing hides a set of brushes. Past a couple of hundred the
  map is unreadable in the 2D panes. The first thing to add.
- **Autosave and recovery.** None. A crash loses the session.
- **Recent files.** None.
- **Cordon.** No way to compile a region of a map.
- **Carve.** Hollow exists; carve does not.
- **Instances / prefabs** (section 11) and a prefab library in the browser.
- **Entity report.** No list of every entity in the map, filterable by class.
- **Check for problems.** Leak detection exists; there is no dialog that
  finds dangling I/O targets, an output aimed at nothing, a missing texture
  or an entity with no name that something targets.
- **Undo history panel.** Undo works; nothing shows it.
- **Vertex/edge editing.** Brushes are plane-defined; no direct vertex
  manipulation.
- **Texture painting.** No brush-based texture painting or blending.
- **Walkmap visualization.** The rule-tint view exists, but there is no
  in-editor preview of the compiled walkmap faces versus what Cleave will
  actually emit.
- **Multi-entity editing.** The Object Properties dialog edits one entity:
  its class keys, its object properties and its output wiring. Selecting
  several and setting a key across all of them is not possible.
- **Play-in-editor.** F9 launches the engine. Launching it as a child with a
  socket and syncing the camera back would be most of what Unity's play
  button is worth.
- **Automated level testing.** No way to script or assert level logic
  in-editor beyond Rhai.

The compilers and Kiln:

- **Incremental builds key on mtimes.** Those break on CI and after a git
  checkout; content hashes do not. A `--watch` mode and a build cache follow
  from having them.
- **Independent stages run in sequence.** Textures, models and sounds do not
  depend on each other.
- **No lint.** `kerosene-tools lint`: materials naming textures that do not
  exist, sounds a map fires that `.kerosnd` does not define, outputs aimed at
  names no entity has. The editor's "check for problems" and this should be
  one function.
- **No map diff.** The formats are text, which is the point; a semantic
  `.keromap` diff -- this brush moved, that output was added -- is something
  no mainstream engine can offer a team using git, and it is a small program.

The engine as a tool:

- **No profiler.** Nothing is instrumented. `r_speeds` reports counts; a
  frame timeline is what finds the hitch. `puffin` draws in egui and drops
  straight into the console overlay; `tracy-client` is the other answer.
- **`stat`-style overlays.** `r_speeds` and `phys_stats` exist; fps, memory,
  audio voices and physics bodies should be the same kind of thing.
- **Hosted docs.** `dev_scripts/docs.sh` builds rustdoc; nothing publishes
  it. The module docs are unusually good and deserve a site.

## 16. What the mainstream engines have, and whether it matters

`positioning.md` argues against chasing Unity, and this document agrees.
But some of what they have is small and worth having, and it is worth being
explicit about which is which.

Worth borrowing:

- **Project templates** and a `new` command.
- **Play-in-editor**, in the child-process form above.
- **Input action maps** with a rebinding screen.
- **Localization** string tables.
- **Build settings per platform**, and named export presets.
- **Prefabs**, as Hammer instances rather than as a scene graph.
- **A profiler in the editor.**
- **Unreal's `stat` commands**, as overlays.

Deliberately not:

- A scene graph, a shader graph, a material graph.
- Heightmap terrain.
- Open-world streaming.
- A garbage-collected scripting runtime. Rhai is small on purpose.
- Character tooling: animation state machines, IK, retargeting, until a game
  in a genre that needs them exists.

## Three gaps worth calling out

1. **The walkmap has a consumer now.** [`NavGraph`](crate::nav) in
   `kerosene-walk` links faces by shared edges and A*-searches them into
   waypoints, so the format is no longer orphaned. The next step is an NPC,
   or a debug overlay that draws a queried path -- the API is real and
   tested, but nothing at runtime calls it.
2. **The solver already does convex hulls; props do not use them.**
   `kerosene-rigid` exposes `add_dynamic_hull`, and world brushes and
   `func_detail` go in as static hulls, so the hull path is real and running.
   But `prop_physics` bodies are built from the model's bounding box
   (`add_dynamic_box_material`), so a barrel collides as a crate. Nothing
   calls `add_dynamic_hull`. Closing this is wiring a hull out of `.keromdl`
   geometry, not new physics.
3. **`$surfaceprop` is driven at runtime.** Traces now report the texinfo they
   hit, the engine resolves that to a material and its `$surfaceprop`, and
   footsteps emit from it. The remaining gap is the *other* side of the coin
   -- impact effects -- and the sound assets themselves, which are content
   rather than code.

## An order

Weighed against `positioning.md`: the shortest path to a shipped movement
shooter or immersive sim, cheapest first.

1. **Interpolation, MSAA, runtime mipmaps, texdata dimensions.** An afternoon,
   and the largest visual change available for the money.
2. **CI.** Three operating systems, `fmt`, `clippy`, the tests. Closes the
   Windows question as a side effect.
3. **Demo record and playback.** Proves determinism, becomes the movement
   solver's regression fixture, and is the ghost system for genre 3.
4. **A game UI layer and save/load.** Menus, an options screen, a HUD,
   ghosts, state across map transitions -- all wait on these two.
5. **Chisel: VisGroups, autosave, instances.** What makes the second real
   map editable.
6. **Weapons and damage.** Hitscan, ammo, `OnDamaged`, decals for the holes.
7. **Steam, Workshop first.** The one thing here no engine of this size can
   match, and the architecture was built for it.
8. **Audio occlusion, reverb, impact sounds.** The horror genre, and the
   physics sandbox sounding like one.

Everything after that -- animation, NPCs, networking -- is in the sections
above and is not smaller for being later.
