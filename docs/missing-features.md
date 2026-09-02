# Missing features

An inventory of what Kerosene does not have yet, split into what the docs
already admit is missing and what the code reveals is missing. Read it as a
candidate roadmap, not a bug list: several of these are deliberate design
choices (a flat entity list, a closed shader set) and several are simply
unstarted.

## 1. Already acknowledged

These come straight from the README's known-limits section and are kept here
for completeness.

- **Networking.** The simulation runs headless (the hard part is done), but
  there is no client/server wire protocol, no snapshot, no prediction, and no
  replication.
- **Skeletal animation.** `.keromdl` carries bones and per-vertex weights and
  Forge preserves them, but nothing plays animation.
- **Chisel 3D view.** Software-rasterised, with correct occlusion but no
  lighting or shadow preview.
- **Texture block compression.** `.kerotex` is uncompressed; no BCn.
- **Audio.** Stereo only. Falloff and panning exist; occlusion, reverb, and
  doppler do not.

## 2. Rendering and visuals

- **Dynamic/real-time lighting and shadows.** Everything is baked by Radiance.
  No shadow mapping, no runtime point/spot lights, no moving light sources.
  `light`, `light_spot`, and `light_environment` are compile-time only.
- **Post-processing.** No bloom, SSAO, color grading, or motion blur; only a
  single `mat_exposure` convar.
- **Anti-aliasing.** `multisampled: false` in the GPU setup; no MSAA/TAA/FXAA.
- **PBR material model.** Materials are a small closed set
  (`lit`/`unlit`/`sky`/`water`/`ui`) with `$basetexture` and `$bumpmap`. No
  metallic/roughness/specular, no emissive maps, no parallax, no per-material
  shader customization.
- **Level of detail.** No LOD for models or geometry.
- **Decals / projected textures.** None.
- **Particles / VFX.** None.
- **Reflections.** No cubemap probes, no SSR, no planar reflections.
- **Dynamic sky / weather / time-of-day.** Sky is a static skybox; sun and
  sky lighting are baked.
- **GPU instancing.** The world draws per material, but repeated `prop_static`
  meshes are not instanced.

## 3. Physics and simulation

- **Rigid-body dynamics.** `kerosene-physics` is the player movement solver
  only. `kerosene-rigid` now wraps Box3D (via `box3d-rust`) and can simulate
  static hulls, dynamic boxes and convex hulls, gravity, resting contact and
  impulses -- but the engine does not yet spawn `prop_physics` entities into it
  or sync their transforms, so nothing in a level uses it yet.
- **Ragdoll / skeletal physics.** None. Box3D has joints, but no skeleton
  attachment.
- **Vehicles / wheeled physics.** None. Box3D has wheel joints, but no vehicle
  controller.
- **Cloth / soft body / fluid simulation.** Water is a volume flag, not
  simulated.
- **Projectiles / ballistics.** No projectile physics. Traces exist for the
  player, but there is no weapon system to use them.
- **Generalized physics queries.** Traces exist, but there is no public
  sweep/overlap API exposed for gameplay beyond movement.

## 4. Animation and characters

- **Skeletal animation playback.** Bones and weights are stored; nothing
  animates them.
- **Animation blending / state machines** (the Animator equivalent). None.
- **Morph targets / blend shapes.** None.
- **Inverse kinematics** (foot placement, look-at). None.
- **Animation retargeting.** None.
- **Third-person character.** Only a first-person controller, which is
  otherwise solid: walk/run/jump/air-strafe, duck, swim, ladders, noclip,
  step-up, health/fall-damage/respawn.

## 5. AI and navigation

- **NPC entities.** No `npc_*` classes; `tools/npcclip` exists as a material
  but nothing is an NPC.
- **Pathfinding.** The walkmap is data only (faces plus rules). There is no
  connectivity graph, no A*/navmesh search, no flow fields, no path
  smoothing. Nothing consumes it yet.
- **Behavior trees / state machines / perception.** No AI decision-making and
  no sight or hearing queries.
- **Crowd / group movement.** None.
- **Dynamic navigation.** The static walkmap cannot reflect a door that is
  currently open or closed.

## 6. Audio

- **Occlusion / reverb / doppler** (acknowledged). A sound through a wall is
  as loud as one in the room.
- **3D spatialization.** No HRTF, no surround.
- **Audio effects / mixing buses.** The mixer is voices into a stereo buffer.
  No EQ, reverb sends, compression, ducking, or effects graph.
- **Streaming audio.** Sounds are decoded whole; no streaming for long
  ambience or music.
- **Footstep/impact effects.** `$surfaceprop` exists in the material format
  specifically to drive footsteps and impacts, but nothing reads it yet.
- **Procedural audio.** None.

## 7. UI and HUD

- **In-game HUD / menus.** The only in-game overlay is the developer console.
  No HUD, no main menu, no pause menu, no dialogue boxes.
- **Gameplay UI toolkit.** No widget system, layout, or theming for shipped
  games (egui is used for tools and the console, not game UI).
- **Localization.** No string tables or translation.
- **Runtime text/font rendering** for gameplay. Only console and editor fonts.

## 8. Input

- **Gamepad / joystick support.** Keyboard and mouse only.
- **Rebindable-input UI.** Console `bind` exists, but no in-game controls
  screen.
- **Touch / mobile input.** None.

## 9. Gameplay systems (beyond the FPS sandbox)

- **Weapons and combat.** No weapons, hitscan, ammo, or reload.
- **Damage model.** Only player fall damage. No damage types, armor, enemy
  health, or hit reactions.
- **Inventory / items / pickups.** None.
- **Objectives / quests / missions.** None.
- **Dialogue system.** None.
- **Save/load.** No game-state serialization; the console `.cfg` persists but
  gameplay state does not.
- **Persistent game flow.** Map transitions exist, but no state carries
  between them (health carries across respawn within a map, not across maps).
- **Difficulty / game settings.** None.

## 10. Content and asset pipeline

- **Prefabs / reusable authored assets.** Brushes are authored per map; no
  prefab library.
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
- **Runtime asset hot-reload.** Chisel reloads textures, but the running
  engine does not hot-reload assets.

## 11. Networking and multiplayer

- **Client/server protocol** (acknowledged). No connection, snapshot, RPC, or
  entity replication.
- **Prediction and interpolation.** None.
- **Dedicated-server story.** `--headless` exists, but there is no network
  stack to serve.

## 12. Platform and distribution

- **Windows/macOS support.** wgpu and winit are cross-platform, but audio is
  ALSA-only on Linux and there is no documented Windows/macOS build or CI
  story.
- **Mobile / console.** None.
- **Steam / launcher integration.** None.
- **Installer / auto-updater.** None.

## 13. Kerosene-specific tooling gaps (editor and compilers)

- **Chisel 3D lighting preview** (acknowledged). Baked lighting is not visible
  until compile and run.
- **Vertex/edge editing.** Brushes are plane-defined; no direct vertex
  manipulation.
- **Texture painting.** No brush-based texture painting or blending.
- **Walkmap visualization.** The rule-tint view exists, but there is no
  in-editor preview of the compiled walkmap faces versus what Cleave will
  actually emit.
- **Prefab/asset browser depth.** The model browser shows previews, but there
  is no saved prefab or variation system.
- **Automated level testing.** No way to script or assert level logic
  in-editor beyond Rhai.

## Two gaps worth calling out

1. **The walkmap is currently orphaned.** It is a solid data foundation, but
   the consumer (NPC pathfinding, a nav query API in `kerosene-engine`) is the
   biggest Kerosene-specific missing piece and the natural next milestone.
2. **`$surfaceprop` is defined but dead.** The material format already
   declares physical surface types explicitly for footstep and impact effects,
   yet nothing emits them. Small, high-polish gap.
