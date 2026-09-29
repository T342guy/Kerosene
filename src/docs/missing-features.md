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
- ~~**No game-state serialization at all.**~~ Fixed: saved games are JSON
  through `serde`, level changes carry state across, and saves mirror to the
  store's cloud. See [Saving and level changes](../gamedev/saving.md).
- **`volume` is the only sound convar an options screen could set.** There
  is no separate music or effects volume.
- ~~**`kerosene-ui` is not a UI toolkit.**~~ Fixed: the tools' egui host is
  now `kerosene-toolui`, and `kerosene-ui` is the game UI (section 8).
- **Chisel is a fifth of the codebase in one crate.** About twenty-eight
  thousand lines now that the GPU viewport has landed in it (as `gpu/`);
  the renderer, the geometry (`brush_edit`, `shapes`, `faces`) and the egui
  layer would split cleanly.

## 2. Already acknowledged

These come straight from the README's known-limits section and are kept here
for completeness.

- **Networking.** The simulation runs headless (the hard part is done), but
  there is no client/server wire protocol, no snapshot, no prediction, and no
  replication.
- ~~**Skeletal animation.**~~ Playback exists: Forge imports skinned glTF
  with its clips, the renderer skins on the GPU, and `prop_dynamic` plays,
  switches and crossfades clips. See section 5 for what is not there.
- **Chisel 3D view.** ~~Software-rasterised.~~ It renders on the GPU now,
  with props drawn as their models and entity helpers (section 19). Still
  no lighting or shadow preview: the view is flat-shaded.
- **Texture block compression.** `.ktex` is uncompressed; no BCn. The
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
  plus `PhysicsWorld`, which wraps Box3D (via `box3d-rust`) and is wired into the
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
  being driven through it. `.kmdl` models render at their simulated pose,
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
- **Projectiles / ballistics.** No projectile physics. The stock weapons are
  hitscan: they trace, mark walls and shove props.
- ~~**Generalized physics queries.**~~ Mostly fixed: `Engine::trace`,
  `trace_ray` and `trace_view` sweep a ray or a box against the world, the
  moving brushes and every prop's body, and say which entity they hit
  (`TraceHit::entity`). Still missing: overlap and radius queries ("what is
  within 200 units"), and traces against a turned prop's real box rather
  than the world box around it.
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
- **Pathfinding.** `NavGraph` links walkable faces that share a stretch of
  edge (T-junctions included, which a compiled floor is full of) and
  A*-searches it for waypoints. The engine loads each map's `.kwalk` with
  it, and `Engine::find_path` and `Engine::nav` are how a game asks. There is
  still no smoothing into a funnel, no flow fields, and no NPC entity to
  steer along the result.
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
  files ship, so a game without its own has silent steps (quietly: a game
  with no footsteps has made a choice).
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
- **Input actions.** `bind` maps keys to commands, and a game's own held
  `+actions` are per-tick state (`engine.input.action_held("zoom")`); the
  bindings are on `Engine`, with `keys_for` for a rebinding screen to show.
  There is still no layer that lets a gamepad and a keyboard both drive
  `+attack`, and no stock rebinding screen.
- ~~**Bindable keys.**~~ Fixed: arrows, the numpad, punctuation, the editing
  keys, mouse 4 and 5, and the wheel (`mwheelup`, `mwheeldown`), under
  Source's names.
- **Touch / mobile input.** None.

## 10. Gameplay systems (beyond the FPS sandbox)

- **Weapons and combat.** A stub exists (`kerosene_game::weapons`): three
  hitscan weapons with ammo, reload, spread and recoil, and a dash on a
  cooldown. A shot marks walls and shoves props; nothing takes damage, and
  there are no projectiles, viewmodels or firing sounds.
- **Damage model.** Only player fall damage. No damage types, armor, enemy
  health, or hit reactions, and no `OnDamaged` output on entities.
- **Inventory / items / pickups.** None.
- **Objectives / quests / missions.** None.
- **Dialogue system.** None.
- ~~**Save/load.**~~ Fixed: `save`, `load`, F5 and F9, `logic_autosave`
  checkpoints. See [Saving and level changes](../gamedev/saving.md).
- ~~**Persistent game flow.**~~ Fixed: `trigger_changelevel` and
  `changelevel` carry the player's health and the game's own state (through
  `Game::save` and `Game::load`) into the next map, lined up by an
  `info_landmark`.
- **Difficulty / game settings.** None.
- **Entity classes.** The set is enough for the demo level and thin for a
  real one. Missing from Source's glue, roughly in the order a mapper hits
  them: `func_movelinear`, `func_tracktrain` and `path_track`, `logic_case`,
  `logic_compare`, `math_remap`, `filter_activator_name` and `_class`,
  `env_fade`, `env_shake`, `trigger_gravity`, `point_viewcontrol`,
  `env_sprite`, `game_ui`. None of these are hard; each is a class in
  `kerosene-game` and a row in the schema -- or in a game's own crate, since
  a game registers classes through the `Game` trait without touching the
  engine. A game's class with a `model` is drawn and collided with when it
  says how (`ClassDef::model(ModelRole::Physics)`); until this audit only the
  three stock `prop_*` names were.

## 11. Content and asset pipeline

- **Prefabs / instances.** Brushes are authored per map; there is no way to
  include one `.kmap` in another. This is not a scene-graph feature --
  Hammer had instances too -- and past a handful of maps the brush workflow
  does not manage without it.
- **Scene graph.** Deliberately a flat entity list (a design choice, listed
  here for comparison with Unity/Unreal).
- **Material editor.** `.kmat` is hand-written KeyValues; no visual
  material graph.
- **Shader graph / custom shaders.** The shader set is closed.
- **Animation import in Forge.** glTF (skins and animations) is in. No FBX,
  no morph targets, no animated scale, and only a model's first skin.
- **Model LOD generation.** None.
- **Terrain tooling.** Brushes only; no heightmap terrain or terrain editor.
- ~~**Level streaming / world partition.**~~ Sections: a visgroup marked as
  streamed is loaded and unloaded around the player by potential
  visibility (see [`architecture.md`](architecture.md#streamed-sections)).
  Still one `.kbsp` per map, compiled and lit as one; not an open world.
- **Visibility control for the mapper.** `tools/hint` and `tools/skip` steer
  Cleave's splits, but there is no `func_areaportal` and no occluder, so a
  door cannot close off what is behind it.
- **Runtime asset hot-reload.** Game UI reloads by itself (`ui_hotreload`),
  and scripts and sound tables on command (`script_reload`, `snd_restart`).
  Materials, textures, models and the map do not, nothing watches files for
  the scripts and sounds, and Chisel-to-engine iteration is F9, which is a
  full restart.
- ~~**Project templates.**~~ Fixed: `kerosene-tools new` makes a game crate
  (a `Game` with a class of its own, its toolset binary, `cargo play`,
  `cargo tools` and `cargo ship`, a project file and a starter map), or with
  `--content-only` a mod. The engine's base content means either runs before
  it has any art of its own.
- **Per-target build settings.** `.kproj` names a content tree and a
  start map, and the window title comes from the game's `LaunchOptions`. An
  icon, default convars and a start map per configuration belong there too,
  along with named ship targets (Godot's export presets) for Kiln.
  `kiln --ship --target <triple>` builds for another platform, but there are
  no presets.

## 12. Networking and multiplayer

- **Client/server protocol** (acknowledged). No connection, snapshot, RPC, or
  entity replication.
- **Prediction and interpolation.** None between machines. On one machine
  the view is interpolated between ticks (section 1).
- **Dedicated-server story.** `--headless` exists, but there is no network
  stack to serve. If the engine goes Steam-first, Steam Datagram Relay is the
  pragmatic transport rather than a hand-written one.

## 13. Platform and distribution

- **Windows/macOS support.** CI compiles the workspace on Windows and
  macOS, but runs the tests and plays a new game only on Linux. Nobody has
  yet played one there by hand.
- **Mobile / console.** None.
- **Crash reporting.** A panic hook writes `crash.log` (section 1); there
  are no minidumps for native crashes and nothing that phones home. `sentry`
  covers both; Breakpad minidumps beside the log are the no-service option.
- **Installer / auto-updater.** None.
- ~~**`build-content.sh` is a shell script.**~~ For a game, `cargo play`
  and `cargo ship` are the build, and they run anywhere Cargo does. The
  script only builds this repository's base content and the base vault.

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
- ~~**Autosave and recovery.**~~ Fixed: `arena.kmap~` every minute, offered
  back when it is newer than the map.
- **Recent files.** None.
- ~~**Cordon.**~~ Fixed: a box that limits what is shown and what Cleave
  compiles, sealed by its own walls.
- ~~**Carve.**~~ Fixed, along with hollow and a clip tool.
- **Instances / prefabs** (section 11) and a prefab library in the browser.
- ~~**Entity report.**~~ Fixed: filterable, and it marks outputs aimed at
  nothing.
- **Check for problems.** Leak detection exists, the entity report finds
  dangling I/O, and "Check for problems" lists every problem in the output
  panel -- broken wires included, by the same check the Outputs tab's
  lights make. It does not yet look for a missing texture, and its lines do
  not lead to what they name.
- ~~**Undo history panel.**~~ Fixed: `Edit → History...`.
- **Vertex/edge editing.** ~~None.~~ Brushes have it: the select tool's
  vertex, edge and face modes, with drag in the flat views, extrude and
  merge, refusing any edit that would make a brush concave. Meshes still do
  not: Chisel draws, picks, moves, resizes, duplicates and deletes them, but
  a mesh is shaped by converting a brush, or by hand in the `.kmap`. No
  bevel yet, for either.
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

- **Incremental builds key on mtimes.** Kiln now skips every stage's
  current outputs (maps also record whether they were built fast or full),
  but by file time, which breaks on CI and after a git checkout; content
  hashes do not. A `--watch` mode and a build cache follow from having them.
- **Independent stages run in sequence.** Textures, models and sounds do not
  depend on each other.
- **No lint.** `kerosene-tools lint`: materials naming textures that do not
  exist, sounds a map fires that `.ksnd` does not define, outputs aimed at
  names no entity has. The editor's "check for problems" and this should be
  one function.
- **No map diff.** The formats are text, which is the point; a semantic
  `.kmap` diff -- this brush moved, that output was added -- is something
  no mainstream engine can offer a team using git, and it is a small program.

The engine as a tool:

- **No profiler.** Nothing is instrumented. `r_speeds` reports counts; a
  frame timeline is what finds the hitch. `puffin` draws in egui and drops
  straight into the console overlay; `tracy-client` is the other answer.
- **`stat`-style overlays.** `r_speeds` and `phys_stats` exist; fps, memory,
  audio voices and physics bodies should be the same kind of thing.
- ~~**Hosted docs.**~~ docs.rs builds the published crate's. Seven of the ten
  stable crates now refuse an undocumented public item (section 18).

## 16. Found in the September 2026 audit

A sweep of the runtime and the tools for what the sections above did not
already list. Nothing in the tree is marked `TODO`, `FIXME` or `todo!()`;
these are gaps found by reading.

Fixed in the same pass:

- ~~**Pausing did not pause.**~~ The pause menu, the console and the Steam
  overlay only showed a layer; the world ran underneath. `sv_pause_on_menu`
  (on by default) now stops the ticks, and a `pause` command does it
  whatever is open.
- ~~**No focus handling.**~~ Out of focus, the game pauses and
  `snd_mute_losefocus` silences it; minimised, it stops drawing.
- ~~**The window was always called "Kerosene".**~~ The title and desktop
  app id are the game's own, from `LaunchOptions`.
- ~~**A failed start said nothing.**~~ A native error box when the renderer
  cannot start, and when a windowed game crashes.

The runtime:

- ~~**Console.**~~ History is kept, Tab completes maps, saves, sounds,
  classes and convars, and there are `maps`, `revert` and
  `host_writeconfig`. `config.cfg` is still written only on a clean quit
  unless asked.
- ~~**Debug commands.**~~ `god`, `buddha`, `notarget`, `give`, `kill`,
  `ent_fire`, `ent_create`, `ent_remove`, `ent_info`, `setpos`, `getpos`,
  `setang`, `restart` and `host_timescale` are in; see
  [the console](console.md). Still no `r_wireframe`, `host_framerate`, or
  `ent_text` drawn in the world (`ent_info` prints to the console).
- **Video.** ~~Fullscreen, `-w`/`-h`/`-fullscreen`, a clamped `cl_fov`,
  `screenshot`~~ are in. Still no render scale, texture-quality or
  anisotropy settings, and no UI text scale.
- **Feel.** Ducking snaps the view; no head bob, view roll, landing punch
  or step smoothing; crouch-jumping does not tuck the feet; no underwater
  tint, fog or muffling; no drowning, though `WaterLevel::Eyes` is there to
  drive it.
- **Rendering.** The worldspawn docs describe fog, and nothing implements
  it. The sky is one equirectangular texture: no cubemap or 3D skybox.
- **Audio.** No music system (playlists, crossfades), no `env_soundscape`,
  no subtitles or closed captions, no per-sound priority, and animated props
  make no sound. ~~Doors and buttons~~ do now: `noise_move`, `noise_stop`
  and `noise_locked`, `door/move` by default.
- **Movers and entities.** A door does not push, stop on or crush a
  blocking player and has no `OnBlocked`; a player on a moving platform is
  not carried; no `parentname`; no `func_door_rotating`,
  `point_template`/`env_entity_maker`, `point_teleport`,
  `trigger_look`/`trigger_proximity`, `game_text` or `sky_camera`.
- **Scripting.** No traces, spawning, or entity angles and velocity in the
  Rhai bindings. ~~Random numbers~~ are in: `rand`, `rand_range`, `rand_int`
  and `pick`, seeded per map.
- ~~**A map that fails to load** leaves a blank world.~~ Its reason is on
  the main menu.

The tools:

- **Chisel.** Cut, copy and paste work (in the editor, not through the system
  clipboard); go-to by id or name and nine camera bookmarks (not saved with
  the map) are in. No texture-lock toggle, though
  `Solid::translate_world_locked` exists; no find and replace. The layout,
  helper mode, shading and fly speed are remembered now (`chisel.layout`);
  the grid is not, and there is no Preferences dialog, keymap file, or light
  theme. Undo keeps a whole copy of the map per step.
- **Validation.** ~~Nothing checks that a connection's input exists on its
  target's class.~~ Fixed: every connection has a light, green, yellow or
  red, with the reason (section 19). There is no sound key type, so sound
  keys have no picker.
- **The output panel** ~~cannot be searched, filtered or copied~~ can now;
  its lines still do not lead to what they name.
- **Other tools.** Timbre, Alchemy and Loupe have no undo. Alchemy reads PNG,
  JPEG and TGA only: no EXR or HDR for skies. Radiance has no named quality
  presets. ~~No `kerosene-tools doctor`~~: there is one now.
- **The texture stage** writes the developer textures into every project's
  `art/` and `materials/`, although the engine's base content already has
  them, because the compilers read tool and sky materials from the project's
  own tree. A new game's `.gitignore` leaves them out; the fix is for the
  compilers to read the base content too.

## 18. Found in the second September 2026 audit

A second sweep, from the game's side this time: what a game crate built on
`kerosene` reaches for and does not find. Struck through is what was fixed
in the same pass; the rest is open.

The game API:

- ~~**Traces never said what they hit.**~~ `Engine::trace`, `trace_ray` and
  `trace_view` return a `TraceHit` with the entity it hit -- a door, a prop
  -- as well as where and which way the surface faced.
- ~~**Only three class names could have a model.**~~ Drawing, animating and
  colliding a model were keyed to `prop_static`, `prop_physics` and
  `prop_dynamic`. A class now says so itself:
  `ClassDef::model(ModelRole::Physics)`.
- ~~**No camera control.**~~ `view_punch`, `screen_shake`,
  `set_fov_override`, `set_view_angles`, and `set_camera` with a
  `CameraOverride` for a cutscene or a death camera. The stock weapons kick.
- ~~**A game could not read its own held actions.**~~ `+zoom` ran as a
  command once a frame; `engine.input.action_held("zoom")` is per-tick
  state now.
- ~~**The bindings belonged to the window.**~~ They are `Engine::input`, so
  a rebinding screen can read and change them, and `bind` works headless.
- ~~**Death was always an instant respawn.**~~ `Game::player_damaged` decides
  how much a hit takes and `Game::player_died` can take the death over,
  leaving the player dead until `Engine::respawn_player`. Maximum health is
  the game's to set.
- ~~**Missing lifecycle hooks.**~~ `frame` (every frame, paused or not),
  `player_spawned`, `can_save`, `map_unloading` and `shutdown`.
- ~~**No debug drawing for a game.**~~ `debug_line`, `debug_box` and
  `debug_point`, for a number of seconds, under `r_debugdraw`. Still no text
  in the world.
- ~~**No seeded random numbers.**~~ `Engine::rng`, seeded by the map and kept
  in saves; `rand`, `rand_range`, `rand_int` and `pick` for scripts. The
  stock weapons' spread still has a generator of its own.
- ~~**Spawning took a class name and nothing else.**~~
  `Engine::spawn_entity(class, &[(key, value)])` reads keyvalues the way
  the map loader does, then runs the spawn handler.
- ~~**The walkmap was never loaded.**~~ See section 6.
- **Class handlers are bare `fn` pointers.** They cannot capture settings or
  reach the game's state; everything goes through a string-keyed
  `HostRequest`.
- ~~**No touch or damage callbacks on a class.**~~ `ClassDef::on_touch` and
  `on_damage`, with `Engine::damage_entity`. Still no use or collision
  callback.
- ~~**Triggers only notice the player.**~~ Spawnflag 8 adds physics props.
  An NPC, when there is one, is still to come.
- **Error types differ by crate.** `platform` and `ui` return
  `Result<_, String>`, the engine mixes that with `anyhow`, the rest use
  typed errors. Unifying them is a breaking change, so it wants doing before
  `1.0.0`.

Rendering:

- ~~**`$alphatest`, `$translucent` and `$nocull` were parsed and ignored.**~~
  On the world, an alpha-tested surface is cut out, a translucent one is
  blended back to front after everything solid, and `$nocull` draws both
  sides. On models the alpha test works, but a translucent model blends
  where it is drawn, among the solid ones and unsorted, and a two-sided
  model is still culled. Umbra and Radiance still treat a translucent brush
  as solid -- it blocks visibility and casts a full shadow -- unless it is
  `func_detail`.
- **No sprites, billboards or beams.** Particles need them first.
- **No render-to-texture or second camera.** Security monitors, mirrors,
  scopes and `sky_camera` all wait on it.
- **No model attachments or hitboxes.** No muzzle point, no held item, no
  headshot.
- ~~**No brightness or vsync at runtime.**~~ `mat_gamma` and `r_vsync`, both
  in the stock options menu and both applied at once.
- ~~**No loading screen.**~~ One is drawn for the frame before a load. The
  load itself still runs on the main thread, so a very long one still holds
  the window on that picture.

Audio:

- ~~**Pausing did not pause the sound.**~~ A paused game holds every sound
  in the world where it is; the interface's clicks play on.
- **No random variants or pitch in `.ksnd`.** One file per sound;
  footsteps get variety by naming (`footstep/<surface>/n`).
- **A playing voice cannot be changed.** No volume, pitch or fade on a
  handle, so no crossfade and no door loop that fades out.
- **No device choice or hot-plug.** The default output always; a device
  that goes away needs `snd_restart`.

Platform and saves:

- ~~**Saves and settings went beside the executable.**~~ They go to the
  player's own directory (`~/.local/share/<game>`, `%APPDATA%\<game>`,
  `~/Library/Application Support/<game>`), and old saves in the content tree
  are still found. `--portable` keeps them in the content tree. `crash.log`
  is still written beside the executable.
- **No local mods folder or `-game` flag.** Workshop items mount; a folder
  of loose mods does not.
- **No save migration.** A newer save is refused and an older one loads as
  it is; nothing lets a game upgrade its own `Game::save` data. No save
  thumbnails.
- ~~**No save or load menu.**~~ The pause menu saves and loads, and the main
  menu loads.

Entities:

- **Missing classes.** ~~`func_breakable`, `func_wall_toggle`,
  `point_hurt`, `item_*` pickups, `game_end`, `player_speedmod`~~ are in,
  with `point_teleport`. Still missing: `func_physbox`, `env_spark`,
  `env_explosion`, and fog and water controllers.
- **Thin common inputs.** `Kill`, `AddOutput` and `FireUser1` to `4` work on
  everything; `SetParent` does not, and `Enable`/`Disable` are each class's
  own rather than universal.

The tools:

- ~~**`cargo xtask bundle --out` emptied whatever it was given.**~~ It clears
  only a directory it made.
- ~~**`cargo xtask help` failed.**~~ It exits 0.
- ~~**The published README's links were broken.**~~ The bundle points them
  at GitHub.
- ~~**`scripts/install-desktop.sh` stopped on a missing icon, and
  `bump-version.sh`'s changelog step needed GNU sed.**~~ Both fixed; the
  bump also moves the changelog's comparison links.
- ~~**No way to clean.**~~ `kerosene-tools clean`, `kiln --clean`.
- ~~**Mistakes on the command line were guessed at.**~~ An unknown command
  gets "did you mean", and Chisel refuses a flag it does not know.
- ~~**`new` could make a game that depends on itself, and made no
  repository, CI or test.**~~ A game called Kerosene is `kerosene-game`;
  `new` runs `git init` and writes a CI workflow, `.gitattributes`,
  `rust-version` and a test of the pickup class.
- ~~**Four copies of "is this output up to date".**~~ One,
  `kerosene_vfs::up_to_date`.
- ~~**"Check for problems" showed one.**~~ It lists them all in the output
  panel.
- ~~**Chisel and Kiln each build the compilers' arguments.**~~ One list,
  `kerosene_vfs::toolchain::MapStages`.
- **No shell completions.** The top-level command line is hand-rolled, so
  clap cannot generate them.
- **The generated game has no licence.** Choosing one is the author's, but
  `new` could ask.
- ~~**No `--watch`.**~~ `kiln --watch` and `play --watch`; the game reloads
  a rebuilt map under `map_autoreload`.

Code health:

- ~~**The stable crates' public items were half undocumented.**~~ `math`,
  `physics`, `script`, `vfs`, `game`, `console` and `platform` are
  documented and warn on anything new that is not. `entity` (about 160
  items), `ui` (about 280) and `engine` (about 150) are not yet, so the lint
  is off for them.
- ~~**Broken links in the crates' own docs.**~~ Every crate's rustdoc builds
  with warnings as errors.
- **Big files.** `engine.rs`'s console commands are in their own module now;
  `kerosene-render/src/gpu.rs` is still 3,200 lines.

## 19. The editor overhaul (September 2026)

Chisel was reworked towards Source 2's Hammer: a GPU 3D view, a layout of
toolbar, per-tool options bar, outliner and docked asset browser, hover
cards, schema-declared entity helpers, a Hammer-style Outputs/Inputs pair
with validation, and vertex/edge/face editing on brushes. See
[Tools](tools.md#chisel--the-world-editor). What doing it turned up:

- **Handles move only in the flat views.** A corner can be picked in 3D but
  not dragged there; the move gizmo still moves whole objects.
- **No marquee for handles.** Corners are picked one click at a time (with
  shift); dragging a box around several is not there yet.
- **No bevel, no edge split, no face split.** Extrude and merge are the
  only operations beyond moving; a face with a new edge across it needs the
  clip tool.
- **Helpers are drawn, not edited.** A spot light's cone cannot be dragged
  wider, a sound's sphere cannot be dragged bigger; the keys are typed.
- **Models are static in the editor.** An animated `prop_dynamic` shows its
  rest pose, and a model's skin or body group is not previewed.
- **`infodecal` and `point_worldpanel` draw an outline**, not the decal's
  material or the panel's layout.
- **The hover card has no material swatch**, only the name.
- **The software rasteriser is kept for tests and thumbnails.** It no
  longer draws models or helpers, so a toolset without a GPU callback path
  shows less than one with it.

## 17. What the mainstream engines have, and whether it matters

`positioning.md` argues against chasing Unity, and this document agrees.
But some of what they have is small and worth having, and it is worth being
explicit about which is which.

Worth borrowing:

- ~~**Project templates**~~ and a `new` command: done, `kerosene-tools new`.
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

1. **The walkmap has a consumer now.** The engine loads each map's
   `.kwalk`, `NavGraph` links its faces (across T-junctions, which it
   used to miss, leaving a real floor with no links at all) and
   `Engine::find_path` answers with waypoints; `Engine::debug_line` can draw
   them. What is missing is the NPC that walks them.
2. **The solver already does convex hulls; props do not use them.**
   `PhysicsWorld` exposes `add_dynamic_hull`, and world brushes and
   `func_detail` go in as static hulls, so the hull path is real and running.
   But `prop_physics` bodies are built from the model's bounding box
   (`add_dynamic_box_material`), so a barrel collides as a crate. Nothing
   calls `add_dynamic_hull`. Closing this is wiring a hull out of `.kmdl`
   geometry, not new physics.
3. **`$surfaceprop` is driven at runtime.** Traces now report the texinfo they
   hit, the engine resolves that to a material and its `$surfaceprop`, and
   footsteps emit from it. The remaining gap is the *other* side of the coin
   -- impact effects -- and the sound assets themselves, which are content
   rather than code.

## An order

Weighed against `positioning.md`: the shortest path to a shipped movement
shooter or immersive sim, cheapest first.

1. ~~**Interpolation, MSAA, runtime mipmaps, texdata dimensions.**~~ Done.
2. ~~**CI.**~~ Done: `fmt`, `clippy`, the layering check and the tests on
   Linux, a build on Windows and macOS, and a new game made and played.
3. **Demo record and playback.** Proves determinism, becomes the movement
   solver's regression fixture, and is the ghost system for genre 3.
4. ~~**A game UI layer and save/load.**~~ Done, with a main menu, a loading
   screen and save and load pages.
5. **Chisel: instances.** VisGroups and autosave are in; this is what
   remains of making the second real map editable.
6. **Weapons and damage.** Hitscan, ammo, `OnDamaged`, decals for the holes.
7. ~~**Steam, Workshop first.**~~ Done, as the `steam` feature.
8. **Impact sounds.** The physics sandbox sounding like one; the room it
   happens in already rings.

Everything after that -- animation, NPCs, networking -- is in the sections
above and is not smaller for being later.
