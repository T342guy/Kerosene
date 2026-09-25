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
- ~~**`kerosene-ui` is not a UI toolkit.**~~ Fixed: the tools' egui host is
  now `kerosene-toolui`, and `kerosene-ui` is the game UI (section 8).
- **Chisel is a quarter of the codebase in one crate.** Eighteen thousand
  lines; worth splitting before the GPU viewport lands.

## 2. Already acknowledged

These come straight from the README's known-limits section and are kept here
for completeness.

- **Networking.** The simulation runs headless (the hard part is done), but
  there is no client/server wire protocol, no snapshot, no prediction, and no
  replication.
- ~~**Skeletal animation.**~~ Playback exists: Forge imports skinned glTF
  with its clips, the renderer skins on the GPU, and `prop_dynamic` plays,
  switches and crossfades clips. See section 5 for what is not there.
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
- **Audio.** Stereo only. Falloff, panning, occlusion, air absorption and
  a per-room reverb the compiler measures exist; doppler does not.

## 3. Rendering and visuals

- **Dynamic/real-time lighting and shadows.** `light_dynamic` (point or spot,
  switchable, movable) and a flashlight are drawn live on top of the bake,
  with clustered shading for up to 32 lights and shadow maps for the nearest
  shadow casters. What is left: no dynamic sun or cascaded shadows (the sun is
  baked whole), no light cookies or IES profiles, no volumetrics, and the
  512² shadow maps are fixed-size rather than a convar.
- **Post-processing.** The scene is HDR (`Rgba16Float`) and tone-mapped once
  per frame (`mat_tonemap`: ACES by default, Reinhard, or none) with a live
  `mat_exposure`. Still no bloom, SSAO, color grading, auto-exposure or
  motion blur; the HDR target is what each of those needs first.
- ~~**Anti-aliasing.**~~ Fixed: `r_msaa` (4x by default) on the HDR target,
  resolved before tone-mapping. No TAA or FXAA.
- **PBR material model.** Materials are a small closed set
  (`lit`/`unlit`/`sky`/`water`/`ui`). They carry colour, normal, roughness,
  emissive, occlusion and metalness maps, plus `$metalness` and
  `$roughnessfactor` scalars, shaded with GGX / Smith / Schlick. What is left:
  no parallax, no per-material shader customization, and diffuse lighting is
  still direction-free, so normal maps on the world are lit by a fixed
  notional direction rather than the real one -- radiosity normal mapping in
  Radiance is the fix.
- **Level of detail.** No LOD for models or geometry.
- ~~**Decals / projected textures.**~~ Mesh decals: cut from the world's
  triangles when placed, lit by the lightmap under them and by dynamic
  lights, from `infodecal`, the `decal` command, scripts and the stock
  weapons (see [Game UI](ui.md#decals)). Not on moving brushes or props, and
  a section streamed in after a decal was placed does not get it.
- **Particles / VFX.** None. Muzzle flash, sparks, dust.
- **Reflections.** `env_cubemap` probes, baked by Radiance and reflected by
  every smooth or metal surface (nearest visible probe, blurred by roughness).
  No parallax correction, no SSR, no planar reflections.
- **Dynamic sky / weather / time-of-day.** Sky is a static skybox; sun and
  sky lighting are baked.
- ~~**GPU instancing.**~~ Fixed: `prop_static` models draw instanced, one
  call per model mesh however many copies there are. (They were not drawn at
  all before, and did not collide; now they do both.)
- ~~**Debug labels.**~~ Fixed: labelled passes and pipelines, and debug
  groups around the world, brush models, props and debug lines.

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

- ~~**Skeletal animation playback.**~~ Fixed for `prop_dynamic`: clips play,
  loop or hold, and crossfade on change. Still missing: a viewmodel -- the
  weapon in front of the camera, the first thing a shooter needs from this --
  and animation events (a footstep sound on frame 12).
- **Animation state machines** (the Animator equivalent). None: two-clip
  crossfades only.
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

- ~~**Occlusion / reverb**~~ Fixed: Resonance measures every room at compile
  time and the mixer has a reverb that reads it; walls muffle, and a sound
  with no way through is silent. See [`audio.md`](audio.md#how-a-room-sounds).
- **Doppler.** A sound moving past you does not bend in pitch.
- **3D spatialization.** No HRTF, no surround.
- **Audio effects / mixing buses.** The mixer is voices into a stereo buffer
  with one reverb bus. No EQ, compression, ducking, or effects graph -- and
  no volume convars for an options screen to drive.
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

- ~~**In-game HUD / menus.**~~ `kerosene-ui`: XML layouts, CSS styles and
  Rhai scripts, with bindings to published game state, hot reload, a stock
  HUD, damage overlay, pause menu with options, and world panels. See
  [Game UI](ui.md). Still missing: a main menu before any map is loaded, and
  dialogue boxes.
- **The attribution screen.** The licence exception requires every game to
  show "Built with Kerosene" when it starts. The engine should draw that
  itself, so a game meets the condition by default and a fork inherits it;
  today each game draws it in `Game::ui`.
- ~~**Gameplay UI toolkit.**~~ See above. What is left: scrolling
  containers, grid layout, and text shaping beyond kerning (no ligatures or
  right-to-left scripts).
- **Localization.** No string tables or translation. Cheap now, painful to
  retrofit.
- ~~**Runtime text/font rendering**~~ for gameplay: a glyph atlas over
  ab_glyph, with `@font-face` for a game's own fonts.

## 9. Input

- **Gamepad / joystick support.** Keyboard and mouse only. `gilrs` is the
  pure-Rust answer; Steam Input replaces it on Steam and adds glyphs.
- **Input actions.** `bind` maps keys to commands, which is half of an action
  map; there is no layer that lets a gamepad and a keyboard both drive
  `+attack`, and no rebinding UI.
- **Touch / mobile input.** None.

## 10. Gameplay systems (beyond the FPS sandbox)

- **Weapons and combat.** A stub exists (`kerosene_game::weapons`): three
  hitscan weapons with ammo, reload and spread, and a dash on a cooldown,
  there to feed the HUD. Nothing takes damage, and there are no projectiles,
  viewmodels or firing sounds.
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
  `kerosene-game` and a row in the schema -- or in a game's own crate, since
  a game registers classes through the `Game` trait without touching the
  engine.

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
- **Animation import in Forge.** glTF (skins and animations) is in. No FBX,
  no morph targets, no animated scale, and only a model's first skin.
- **Model LOD generation.** None.
- **Terrain tooling.** Brushes only; no heightmap terrain or terrain editor.
- ~~**Level streaming / world partition.**~~ Sections: a visgroup marked as
  streamed is loaded and unloaded around the player by potential
  visibility (see [`architecture.md`](architecture.md#streamed-sections)).
  Still one `.kerobsp` per map, compiled and lit as one; not an open world.
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

- ~~**Steamworks**~~ In, as the opt-in `steam` feature (see
  [Steam](../gamedev/steam.md)). It covers:
  - init and the relaunch through Steam
  - the overlay, which pauses the game
  - achievements (with progress), stats and leaderboards
  - rich presence, DLC checks and the cloud file API
  - Workshop items mounted as VFS layers, with `kerosene-tools workshop
    upload` for putting a `.vault` up

  All of it is reachable from entity I/O (`logic_achievement`, `logic_stat`,
  `logic_leaderboard`, `logic_richpresence`, `logic_platform`), from Rhai (the
  `platform` object, alias `steam`) and from game code. `kiln --ship --steam`
  installs Valve's redistributable with an rpath and writes the SteamPipe
  scripts.

  Still missing:
  - Steam Input
  - lobbies and Steam Datagram Relay
  - leaderboard downloads
  - Workshop browsing in-game
  - uploading from Chisel rather than the command line
- ~~**A platform trait**~~ `kerosene-platform`'s `Backend`: Steam is one
  implementation and the offline stand-in another, and no game code names
  either. GOG Galaxy and Epic Online Services are not written.
- **Discord** rich presence (`discord-rich-presence`). Trivial.
- **Crash reporting** (section 13).
- **itch.io** as a Kiln ship target, via butler.

## 15. Kerosene-specific tooling gaps (editor and compilers)

Chisel:

- **3D lighting preview** (acknowledged). Baked lighting is not visible
  until compile and run.
- ~~**VisGroups.**~~ Fixed: a VisGroups tab with nested user groups and
  automatic ones, quick-hide, groups, and object colours.
- **Autosave and recovery.** None. A crash loses the session.
- **Recent files.** None.
- ~~**Cordon.**~~ Fixed: a box that limits what is shown and what Cleave
  compiles, sealed by its own walls.
- ~~**Carve.**~~ Fixed, along with hollow and a clip tool.
- **Instances / prefabs** (section 11) and a prefab library in the browser.
- ~~**Entity report.**~~ Fixed: filterable, and it marks outputs aimed at
  nothing.
- **Check for problems.** Leak detection exists and the entity report finds
  dangling I/O; there is still no one dialog for a missing texture or an
  entity with no name that something targets.
- ~~**Undo history panel.**~~ Fixed: `Edit → History...`.
- **Vertex/edge editing.** Maps can hold polygon meshes now, and Chisel draws,
  picks, moves, resizes, duplicates and deletes them, and converts brushes to
  them (Tools → Convert to mesh). What it cannot do yet is edit one: no
  vertex, edge or face selection, extrude or bevel. Until it can, a mesh is
  shaped by converting a brush, or by hand in the `.keromap`.
- **Texture painting.** No brush-based texture painting or blending.
- **Walkmap visualization.** The rule-tint view exists, but there is no
  in-editor preview of the compiled walkmap faces versus what Cleave will
  actually emit.
- ~~**Multi-entity editing.**~~ Fixed: the Object tab and the properties
  window edit every selected entity, brush or face together, with a key
  they disagree on marked and left alone until typed over.
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
- Open-world streaming. Sections of one map stream; a world of many maps
  does not.
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
5. **Chisel: autosave, instances.** VisGroups are in; these two are what
   remains of making the second real map editable.
6. **Weapons and damage.** Hitscan, ammo, `OnDamaged`, decals for the holes.
7. **Steam, Workshop first.** The one thing here no engine of this size can
   match, and the architecture was built for it.
8. **Impact sounds.** The physics sandbox sounding like one; the room it
   happens in already rings.

Everything after that -- animation, NPCs, networking -- is in the sections
above and is not smaller for being later.
