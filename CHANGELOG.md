# Changelog

Every notable change to Kerosene. The version is the `kerosene` crate's and
follows [Semantic Versioning](https://semver.org) on that crate's public API;
[Versioning](src/docs/versioning.md) says exactly what that covers. The
format is [Keep a Changelog](https://keepachangelog.com).

Until `1.0.0`, pre-releases (`-aN` alpha, `-bN` beta, `-rc.N`) may break
the API between one another. Breaking changes are listed under **Changed**
with what to do about them.

## [Unreleased]

### Added
- The toolset window is rebuilt around a sidebar of seven tabs -- Home,
  Assets, Editor, Models, Sound, Build, Archive (`ctrl-1`..`ctrl-7`) -- a top
  bar with a search box, Play and the running jobs, and a refreshed theme
  that Chisel, Timbre and Loupe share.
- A command palette (`ctrl-P` or `ctrl-K`): fuzzy search over every tab,
  job, map, model, sound and recent project.
- An Assets tab: every file in the content tree by category, searchable and
  sortable, with whether each source's compiled form is there and up to
  date; double-click opens a map, model or sound in its tool.
- A start page: recent projects, and forms to open a project, start a new
  game (`new`) or make a project of a folder (`init`) without a terminal.
  Projects can be switched without restarting; an unsaved map is asked
  about first.
- Build jobs can be cancelled, and show their elapsed time. The Build tab
  offers *rebuild everything*, *ignore leaks* and *dry run*, and a Clean
  button. The Archive tab lists the archive's contents as a table and says
  when it is older than what it packs.
- The output panel counts each log's errors and warnings, filters by text or
  to problems only, copies a whole log, and draws only the rows on screen.
- `kerosene-tools --example shot` renders any tab off screen to a PNG.
- Chisel's 3D view renders on the GPU, through a paint callback in the
  toolset's own window: it keeps up with a whole level while flying, props
  are drawn as their models, and a selection is outlined faintly through
  walls. The software rasteriser remains for tests and thumbnails.
- Entity helpers, declared in a `.kdef` with `helper { "type" ... }`:
  `model`, `lightradius`, `lightcone`, `sphere`, `frustum`, `direction`,
  `line` and `rect`, drawn in the 3D and flat views for the selection, every
  entity, or none. The stock classes declare theirs -- light cones and
  radii, sound radii, panel rectangles, facing arrows. `ClassSpec::helpers`,
  `HelperSpec` and `HelperKind` in `kerosene::entity::schema`.
- Chisel's layout follows Hammer 5: a toolbar for what applies everywhere,
  an options bar for the tool in hand, an outliner above the properties, and
  the asset browser docked along the bottom -- whose Models tab arms the
  entity tool with the model clicked. One, two or four views. The layout is
  remembered between sessions (`chisel.layout` in the user data directory).
- Hover cards in every view: what an entity is, its name, keys and wiring;
  a brush's size and the face's material.
- Entities can be picked in the 3D view (they could not be), and a
  double-click opens the properties of what it selected.
- Entity I/O in Hammer's form: an Outputs list with a green, yellow or red
  light on every connection saying whether it will do anything and why, a
  form for the picked one that offers the target's actual inputs with their
  help, copy and paste of connections, and an Inputs tab listing everything
  that fires at the entity. The selected entity's wiring is drawn in the
  views. `Map → Check for problems` lists every broken wire.
- Brush modelling: the select tool's vertex, edge and face modes
  (`Shift+2`..`Shift+4`), with handles dragged in the flat views, extrude,
  and merge; an edit that would make a brush concave is refused with the
  reason. `F` frames the selection; `F1` shows every shortcut.
- `kerosene_toolui::App::gpu_ready`, telling an app the window has a GPU and
  what egui renders into.
- Chisel examples `gpu_shot` and `ui_shot`: the GPU 3D view, and the whole
  editor window, rendered off screen to a PNG.

- `kiln --watch` builds again whenever a source changes, and
  `play --watch` does it while the game runs. The engine's new
  `map_autoreload` convar, which `play --watch` turns on, reloads a rebuilt
  map with the player where they stood.
- `kerosene::vfs::toolchain::MapStages`: the compilers a map goes through
  and what each is told, shared by Chisel's compile and Kiln so the two
  cannot build a map two ways.
- `Engine::teleport_player`, and `Vfs::disk_path` for the loose file behind
  a virtual path.
- Developer commands: `god`, `buddha`, `notarget`, `kill`, `give`,
  `getpos`, `setpos`, `setang`, `ent_fire` (by name, class or `!picker`),
  `ent_create`, `ent_remove`, `ent_info`, `restart`, `maps`, `revert`,
  `host_writeconfig`, `host_timescale` and `screenshot`. See the new
  [console](src/docs/console.md) page.
- Tab completes arguments as well as commands: maps, saves, sounds, classes
  and convars. `Console::register_completer` gives a game's commands the
  same, and `Console::complete_line` is what the overlay asks.
- The console's history is kept between sessions, in
  `cfg/console_history.txt`.
- `r_fullscreen` (window, borderless, exclusive), in the options menu too,
  and the launch flags `-w`, `-h`, `-fullscreen` and `-windowed`.
- `Engine::kill_player`, `Engine::aimed_entity`, `Engine::write_config`,
  `Engine::save_console_history`, and `god`, `buddha` and `notarget` for a
  game to read.
- A front end. A windowed game opens on a main menu (`ui/menus/main.kui`,
  named by `ui_mainmenu`) with New Game, Load, Options and Quit, behind a
  two-second "Made with Kerosene" splash (`ui_splash`, `-nosplash`). New
  Game loads the project's start map. A `+map`, `-nomenu`, a headless run and
  `kerosene-tools play` go straight in as before.
- The pause menu has Save and Load pages listing the saves, and Quit to
  menu. The options page is `ui/menus/options.kui` and the save list
  `ui/menus/saves.kui`, each included by both menus.
- A loading screen (`ui/menus/loading.kui`, `ui_loading`), drawn for the
  frame before a map or save loads so the window does not freeze on the last
  picture.
- A map that fails to load says why on the main menu (`error.message`), not
  only in the console.
- `newgame` and `disconnect`; `Engine::new_game`, `new_game_map`,
  `unload_map`, `end_game`, `show_splash`, `set_loading_screen` and
  `publish_saves`; `EngineConfig::new_game_map`.
- Gameplay classes: `func_breakable`, `func_wall_toggle`, `point_hurt`,
  `point_teleport`, `item_healthkit`, `item_generic`, `player_speedmod` and
  `game_end`, with their schema for Chisel. `FireUser3`/`FireUser4` and
  `OnUser3`/`OnUser4` on every entity.
- Damage and touch for classes: `ClassDef::on_damage` and
  `ClassDef::on_touch`, with `Engine::damage_entity`, which also fires
  `OnDamaged` on anything it hits. The stock weapons damage what they shoot,
  so glass breaks.
- A trigger with spawnflag 8 notices physics props as well as the player.
- `kerosene-resource`: the one compiled resource container (a header with
  the kind and a hash of the source, then typed blocks: the payload, the
  other resources it needs, and how it was compiled), `Resource<T>`
  handles with a cache that loads now or later and reloads in place, and
  the table of asset types. See `src/docs/formats.md`.
- `cargo xtask layers`, run by CI: every crate depends only on the layers
  below it, and subsystems never on each other (`src/devnotes/crate-map.md`).

- `kerosene-reflect` and `kerosene-ecs`: entity fields are declared once, as
  fields of a component with `Key`, `Label`, `Help`, `Widget`, `Hidden`,
  `Transient` and `Networked` attributes. The map loader, saves, the
  editor's keys and scripts all read that declaration. Entities live in a
  pinned `bevy_ecs` behind `kerosene_ecs::prelude`.
- `EntityWorld::keyvalue`, `keyvalue_f32`/`_i32`/`_bool`/`_text`,
  `set_keyvalue`, `keyvalues` and `is_disabled`: a key wherever it lives.

### Changed

- `kerosene-render`'s camera, world mesh, lightmap, lights, probes, decals
  and BRDF tables moved to `kerosene-scene`. `kerosene::internals::render`
  still exports them.
- `kerosene-rigid` is folded into `kerosene-physics` as `physics::rigid`, and
  `RigidWorld` is now `PhysicsWorld`. `kerosene::internals::rigid` is gone.
- Device creation, surface configuration and frame read-back moved into
  `kerosene-rhi`.
- **Entity classes are components.** `ClassDef::component` gives a class the
  components its entities carry; the stock game's classes keep their keys
  and state in them rather than in loose fields. Read a key with
  `EntityWorld::keyvalue*`, not `Entity::fields`, when the class may be a
  component's. Scripts' `set` and `ent_info` go through the same path.
- **Materials are compiled.** Alchemy (and so `kiln` and Chisel) compiles
  each `.kmat` to a `.kmat_c` beside it, and the engine loads that; `.kmat_c`
  is packed instead of `.kmat`. A project not rebuilt since still runs from
  its `.kmat` files, with a warning each. `kerosene_asset::material_path`
  now names the `.kmat_c`; `material_source_path` names the source.
- `.ktex` and `.kmdl` are written in the resource container. Files written
  before still load.
- **Breaking:** `EntityWorld::load_from_bsp(&Bsp)` is
  `load_from_lump(&KeyValues, &[Aabb])`: pass `bsp.entities_kv()?` and
  `bsp.model_bounds()`.
- **Breaking:** `UiRenderer::upload_atlas` takes the atlas's pixels, and
  the caller checks its `dirty` flag. `kerosene_config::gpu` is
  `kerosene_rhi::gpu`. The UI draw list is in `kerosene-scene`, and
  `Connection` in `kerosene-kv`; `kerosene-ui` and `kerosene-map` re-export
  them.
- CI is five jobs that must all pass: checks and tests on Linux, a build on
  Windows and macOS, the bundled crate with a new game played headless, the
  book, and `cargo deny`. The MSRV and semver jobs are release steps now
  (`src/docs/releasing.md`).
- A windowed game with no `+map` opens on the main menu rather than on the
  project's start map. `-nomenu` restores the old behaviour.
- `game.title` is published to the UI, and the pause menu shows it rather
  than "Kerosene".
- `cl_fov` takes 50 to 130.
- Turning `sv_cheats` off puts every cheat convar back to its default.
- **Breaking:** `kerosene::tools::Tab::Project` is `Tab::Home`, and the tabs'
  `ctrl` digits follow their new order. `ProjectAction` is replaced by
  `kerosene::tools::Action`.
- **Breaking:** every file extension is shorter -- a `k` and what the file
  is -- and the old names are no longer read. Rename existing files:

  | old | new | old | new |
  |---|---|---|---|
  | `.keromap` | `.kmap` | `.kerodef` | `.kdef` |
  | `.kerobsp` | `.kbsp` | `.keroaud` | `.kaud` |
  | `.keromdl` | `.kmdl` | `.kerosnd` | `.ksnd` |
  | `.keromat` | `.kmat` | `.kerosave` | `.ksav` |
  | `.kerotex` | `.ktex` | `.kerowalk` | `.kwalk` |
  | `.keroproj` | `.kproj` | `.keroprt` | `.kprt` |
  | `.keroui` | `.kui` | `.keroleak` | `.kleak` |
  | `.kerocss` | `.kcss` | `.kerobuild` | `.kbuild` |
  | `.keroscript` | `.kscr` | `.kconfig` | `.kcfg` |

  Compiled files (`.kbsp`, `.ktex`, `.kaud`, ...) are simply rebuilt by
  `kerosene-tools kiln`; hand-written ones and saves need renaming, and a
  `.kui` that includes a stylesheet or script by name needs the name inside
  it changed too. `.vault` is unchanged. Every extension is now in one table,
  `kerosene::vfs::ext`.
- **Breaking:** `kerosene::entity::schema::ClassSpec` has a `helpers` field,
  so a `ClassSpec` built with a struct literal needs `..Default::default()`.
- Chisel's inspector tabs are gone: tool settings moved to the options bar,
  materials to the asset dock, visgroups beside the outliner. `M` shows the
  asset dock rather than a window.
- The old "when this happens, do, then" wiring editor is replaced by the
  Outputs list and form.
- Entity names are written beside their icons in the flat views only when
  zoomed in far enough to read, or when selected.

### Fixed
- `cargo xtask bundle` no longer mangles a crate path after `..` (a struct
  update like `..kerosene_vfs::toolchain::MapStages::new(..)`), which broke
  the bundle.
- The release workflow stops with a clear message when a release for the
  tag already exists, rather than failing part-way through an upload.
- The link checker reads `[text](<path with spaces>)` links.
- Timbre's settings are `sound/timbre.kcfg`. They were `timbre.kerobuild`,
  which `kiln --clean` deleted as a build stamp and `.gitignore` kept out of
  version control.
- Chisel matched output targets to entity names case-sensitively when
  offering inputs; the engine does not, and neither does the editor now.
- `condump` wrote to any path it was given, and scripts can run console
  commands, so a map could overwrite a file of the player's. It takes a name
  and writes into the player's directory through the VFS.
- `math_counter` crashed on `min` above `max`, or a limit that is not a
  number. Such limits are swapped or ignored.
- A spawn's, a loaded save's or a level change's facing was lost on the
  frame it happened, overwritten by the view the host had read before it.
- A screen shake or view punch begun late in one map lasted into the next,
  for as long as the old map's clock had run.
- Entities' sound handles were saved with them, so a loaded save stopped
  the wrong sounds. The engine keeps them, and forgets them on a load.
- A door reopened by hand before its `wait` ran out was shut early by the
  return its first opening had queued.
- An entity killed mid-tick still took inputs and ran its think until the
  end of the tick.
- A save written by `logic_autosave` recorded the clock one tick ahead of
  the physics it held; it is made at the end of the tick now.
- A save listing a free slot twice was loaded, and handed that slot to two
  entities.
- A clamped convar took `inf` and `nan`. It keeps its value instead.
- An alias defined as `alias hi echo "one; two"` lost its quotes and ran as
  two commands. A single quoted body (`alias go "a; b"`) still runs as two.
- `rand_int` panicked on a range wider than `i64` holds.
- A layout's inline script or style with a non-ASCII character in it could
  crash the UI parser.
- One sound placed at a position that was not a number silenced every other
  sound until it stopped.
- A Steam stat store that failed was never tried again.
- A map with more than 42 `env_cubemap`s crashed the renderer. The first 42
  are used, and Radiance warns about the rest.
- A texture claiming more mip levels than its size allows was accepted and
  then refused by the GPU; it is refused on load.
- Kiln:
  - Editing a material or texture now rebuilds the maps; it used to leave
    them "up to date" with the old face sizes and acoustics.
  - A leak trace left by an earlier build made any later failure look like
    a leak and hid its error.
  - `--ignore-leaks` always ended in a failed build.
  - `--ship` could refuse an archive `pack` had just called up to date.
  - A pack cut short left a truncated archive that the next build called up
    to date. Archives are now written to a scratch file and renamed into
    place, and an archive counts as current only if it holds exactly what
    the tree would pack, so deleted files leave it too.
  - A map rebuild that stopped part way kept the last build's stamp and was
    skipped next time.
  - `--clean` missed compiled files with capitalised extensions, and
    followed symlinked directories out of the project.
- An archive whose header claimed more entries than its directory holds
  could make a tool try to allocate hundreds of gigabytes.
- `Vfs::list` found nothing in archives for a directory or extension typed
  with capitals.
- `vault pack --list` passed a list line whose only match was excluded,
  leaving a hole in the archive with no error.
- The Assets tab looked for meshes under `models/`; Kiln builds them from
  `art/` into `models/`, and the tab says so now.
- `toggle` and `incrementvar` changed cheat convars with `sv_cheats 0`.
- A disabled `func_brush` stopped being solid but was still drawn.
- A style binding that read something not yet published applied a broken
  value and warned (the base HUD's dash ring did this in any game without a
  dash). It is left alone until there is a value, and the dash panel is
  hidden when there is no dash.
- In the toolset, New map did nothing when the open map had unsaved changes,
  and opening a map from the palette or Assets threw them away. Both ask
  first, as switching projects already did.

## [1.0.0-a3] - 2026-09-26

### Added
- `cargo xtask publish`, from the repository root: refuses a version already
  on crates.io, warns about uncommitted or unpushed work, a forgotten version
  bump or a missing changelog section, then bundles, publishes and offers to
  tag the release. `--dry-run` does everything but the upload.
- Traces for games: `Engine::trace`, `trace_ray` and `trace_view` return a
  `TraceHit` that says which entity was hit -- a door, a prop -- as well as
  where and the surface's normal.
- `ClassDef::model(ModelRole)`: any class can have a model the engine draws,
  animates and collides with, not only `prop_static`, `prop_physics` and
  `prop_dynamic`. `ClassRegistry::model_role` answers for a class.
- `Game` hooks, all with defaults: `frame` (every frame, paused or not),
  `player_damaged` (how much a hit takes), `player_died` (take a death
  over), `player_spawned`, `can_save` (refuse a save), `map_unloading` and
  `shutdown`. `Engine::respawn_player`, `player_alive`,
  `player_max_health` and `set_player_max_health`.
- Camera control: `Engine::view_punch`, `screen_shake`, `set_fov_override`,
  `set_view_angles`, and `set_camera` with a `CameraOverride` for a cutscene
  or a death camera; `Engine::view_camera` is what gets drawn. The stock
  weapons kick, and push the props they hit.
- `Engine::spawn_entity(class, &[(key, value)])` and
  `EntityWorld::spawn_with`: an entity with keyvalues, read as a map's are,
  and spawned. `EntityWorld::apply_keyvalue`.
- `Engine::debug_line`, `debug_box` and `debug_point`, drawn for a number of
  seconds while `r_debugdraw` is on.
- A seeded random number generator, `kerosene::math::Rng`: `Engine::rng()`
  is seeded by the map and kept in saved games. Scripts get `rand()`,
  `rand_range(lo, hi)`, `rand_int(lo, hi)` and `pick(array)`.
- Navigation at run time: the engine loads each map's `.kerowalk`;
  `Engine::nav` and `Engine::find_path` answer with waypoints.
- Saves and `config.cfg` go to the player's own directory
  (`~/.local/share/<game>`, `%APPDATA%\<game>`,
  `~/Library/Application Support/<game>`); `--portable` keeps them in the
  content tree. Saves already in the content tree are still found.
  `EngineConfig::user_dir` and `with_user_dir`,
  `kerosene::vfs::user_data_dir`.
- Pausing holds every sound in the world where it is; the interface's play
  on.
- Doors and buttons make noises: `noise_move`, `noise_stop` and
  `noise_locked`, with `door/move` for a door by default. An entity sound on
  a brush entity comes from the middle of the brush.
- More keys can be bound: arrows, the numpad, punctuation, the editing keys,
  `mouse4`, `mouse5`, and the wheel as `mwheelup` and `mwheeldown` (the
  stock game puts `invprev` and `invnext` there).
- The bindings are `Engine::input`, so a game can read and change them;
  `InputSystem::keys_for` for a rebinding screen, and
  `InputSystem::action_held` for a game's own `+action`. `bind` works in a
  headless run.
- Materials: `$alphatest` cuts a surface out (`$alphatestreference`, 0.5 by
  default), `$translucent` blends it back to front after everything solid,
  and `$nocull` draws both sides. `Material::is_blended`,
  `is_alpha_tested` and `alpha_test_reference`.
- `mat_gamma` and `r_vsync`, applied at once, and both in the stock options
  menu as Brightness and Vertical sync.
- `kerosene-tools doctor` checks the machine: Rust, the Linux sound
  headers, the GPU, the project and the toolchain.
- `kerosene-tools clean` and `kiln --clean` delete what the content build
  wrote, and nothing else. `kerosene::vfs::COMPILED_EXTENSIONS` says what
  that is, and `kerosene::vfs::up_to_date` is the build's freshness check.
- `kerosene-tools new` runs `git init` (not with `--no-git`) and writes a CI
  workflow that tests and plays the game on three platforms,
  `.gitattributes`, `rust-version`, and a test of the game's class.
- An unknown `kerosene-tools` command is answered with the one it nearly
  was.
- "Check for problems" in Chisel lists every problem in the output panel.
- `kerosene_map::starter::room`, the room a new game starts with and the
  engine falls back to.

### Changed
- **Breaking:** the demo level has moved to its own repository,
  [kerosene-demo](https://github.com/t342guy/kerosene-demo), as a game crate
  on the published `kerosene`. The base content's map is now a plain room:
  `kerosene::engine::base::DEMO_MAP` is `FALLBACK_MAP`, `"kerosene_room"`.
  A game that opened `kero_start` by name should ship its own map.
- **Breaking:** `Engine::trace_view` returns `Option<TraceHit>` instead of
  a tuple: `hit.pos`, `hit.normal` and `hit.distance` for what were its
  three parts.
- **Breaking:** `ClassDef` is `#[non_exhaustive]`: build it with
  `ClassDef::new` and its methods, as every example does.
- **Breaking:** where saves and `config.cfg` are written; see Added. Nothing
  is lost -- old saves are still read -- but a game's player will find new
  saves in a new place.
- `point_worldpanel`'s default layout is `ui/panels/status.keroui`.
- Footstep sounds a game does not have are skipped quietly rather than
  warned about.
- A game called "Kerosene" (or "test", "core"...) gets a package name that
  does not collide: `kerosene-game`.
- The engine's console commands are registered in their own module; the
  seven smaller stable crates (`math`, `physics`, `script`, `vfs`, `game`,
  `console`, `platform`) are fully documented and warn on anything new that
  is not.

### Fixed
- `Angles::slerp` returns its endpoints exactly, so an interpolated door on
  a tick boundary is where it is on every platform (it was a hair off on
  macOS).
- The base vault's freshness test passes on a Windows checkout with CRLF
  line endings, and `.gitattributes` keeps text LF everywhere.
- Pathfinding linked only faces that shared a whole edge; a compiled floor
  meets itself at T-junctions, so real maps had no links at all. Faces that
  share part of an edge are linked through the middle of it.
- CI's new-game job read the log from stdout; the engine logs to stderr.
- The release workflow makes a draft release, since the repository's
  releases are immutable and cannot take assets once published.
- `cargo xtask bundle --out <dir>` refuses to empty a directory that is not
  a bundle; `cargo xtask help` exits 0; the bundle's README and changelog
  link to GitHub rather than to files that are not in the package.
- `scripts/install-desktop.sh` finds the 256px icon;
  `scripts/bump-version.sh` works with macOS's sed and moves the
  changelog's comparison links.
- Chisel refuses a flag it does not know rather than ignoring it.
- Broken links in the crates' own docs, and in the book.

## [1.0.0-a2] - 2026-09-26

### Added
- `kerosene-tools new <dir>` makes a game: a Cargo package depending on
  `kerosene`, a `Game` with a class of its own, the game's own toolset
  binary, a project file and a starter map. `cargo play`, `cargo tools` and
  `cargo ship` are set up as aliases. `--content-only` makes a project with
  no Rust.
- `kerosene-tools play`: build whatever content changed, then build and run
  the game. It is what `cargo play` runs.
- The engine's base content, compiled into every game: developer and tool
  textures, the stock props, sounds, HUD and menus, and the `kero_start`
  demo map. A game with no content of its own starts and runs; a game's own
  files override it.
- Kiln skips models, maps and the archive when they are newer than their
  sources, as it already did for textures and sounds. It also takes
  `-j/--jobs` and `-q/--quiet`, and `--force` now rebuilds every stage.
- `vault pack --list <file>` packs only what a list names.
- `pause` command, and `sv_pause_on_menu` (default on): the world stops
  while the pause menu, the console or the Steam overlay is open, or the
  window is in the background. `snd_mute_losefocus` (default on) silences
  a game in the background. Nothing is drawn while the window is minimised.
- The window's title and desktop app id are the game's, from
  `LaunchOptions`. The `version` command and the log name the game and its
  version beside Kerosene's.
- A native error box when the renderer cannot start, and when a windowed
  game crashes.
- `kerosene::VERSION`, `Engine::time`, `tick_count`, `quit`,
  `quit_requested`, `has_level`, `map_name` and `vfs`.
- `EngineConfig` setters: `with_content`, `with_map`, `with_command`,
  `with_audio`, `with_log`, `with_platform`, `with_base_content`.
- `Archive::from_static` and `Vfs::mount_static`, for archives compiled into
  a program.
- `CHANGELOG.md`, the Versioning page, and `scripts/bump-version.sh`.

### Changed
- **Breaking:** `LaunchOptions` is built with
  `LaunchOptions::new(name, version)`, and `.app_id()`, `.extra_help()`,
  `.args()`. It no longer implements `Default`, so a game always states its
  own version. Replace `LaunchOptions { name, version, ..Default::default() }`
  with `LaunchOptions::new(name, version)`.
- **Breaking:** `kerosene_tools::Options` likewise:
  `Options::new(name, version).schema(..).game(..)`.
- **Breaking:** `EngineConfig` is `#[non_exhaustive]`. Start from
  `EngineConfig::default()` and use the setters, or assign fields.
- **Breaking:** `Engine`'s `time`, `tick_count` and `should_quit` fields are
  now the methods above. `level`, `physics`, `animations`, `script`, `vfs`
  and `log` are no longer public fields. `vfs()` is the stable way to reach
  the content; the others are `#[doc(hidden)]` accessors, outside the SemVer
  promise.
- **Breaking:** the engine's inner crates moved to `kerosene::internals`:
  `asset`, `audio`, `bsp`, `config`, `kv`, `map`, `render`, `rigid` and
  `walk`. `kerosene::internals::bsp` where it was `kerosene::bsp`.
- **Breaking:** `#[non_exhaustive]` on `PlatformAction`, `StatKind`,
  `ScriptError`, `SpawnError`, `SchemaError`, `VfsError` and `ArchiveError`,
  so later releases can add to them.
- Kerosene is published as one crate, `kerosene`: `cargo xtask bundle` folds
  every workspace crate into it as a module, with the tools behind the
  `tools` feature and the licence texts included. `cargo install kerosene
  --features tools` installs the toolset. The workspace's packages are
  `publish = false`; the tool packages are named `kerosene-chisel`,
  `kerosene-kiln` and so on, their libraries keeping their short names.
- Every Kerosene crate names its siblings at exactly the same version.
- A launch with no content tree runs on the base content instead of warning,
  and a launch with no map opens the demo map.

### Fixed
- The console's own log lines are recognised by an explicit log target
  rather than by module path, so they are not echoed twice in the published
  crate.
- The licence identifier is now
  `GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0`, in `Cargo.toml`
  and every source file. `LicenseRef-` is not allowed after `WITH` in SPDX,
  so crates.io and licence auditors could not read the old spelling, and the
  Exception's permission to link Kerosene statically went unseen. The terms
  are unchanged.
- Opening the pause menu, the console or the Steam overlay did not pause the
  game: the world kept running underneath.

## [1.0.0-a1] - 2026-09-25

The first version numbered by the `kerosene` crate's API.

### Added
- Steamworks, behind the `steam` feature: achievements, stats,
  leaderboards, rich presence, cloud files, DLC and Workshop mounting. Map
  scripts, UI scripts and entities (`logic_achievement`, `logic_stat`,
  `logic_leaderboard`, `logic_richpresence`, `logic_platform`) reach it all.
  `kiln --ship --steam` installs Valve's redistributable beside the game.
- Saved games and level changes: `save`, `load`, F5/F9, `changelevel`,
  `trigger_changelevel`, `info_landmark`, `logic_autosave`, autosave on
  arrival, and saves mirrored to the store's cloud. `Game::save` and
  `Game::load` keep a game's own state.
- The game UI framework: layouts, stylesheets, a data store, UI scripts,
  world panels, and the stock HUD and pause menu.
- Skeletal animation: glTF import, GPU skinning, `prop_dynamic`.
- Mesh world geometry, instanced static props, dynamic lights with clustered
  shading and shadow maps, HDR, MSAA, GGX/metalness shading and cubemap
  probes.

[Unreleased]: https://github.com/t342guy/kerosene/compare/1.0.0-a3...HEAD
[1.0.0-a3]: https://github.com/t342guy/kerosene/compare/1.0.0-a2...1.0.0-a3
[1.0.0-a2]: https://github.com/t342guy/kerosene/compare/1.0.0-a1...1.0.0-a2
[1.0.0-a1]: https://github.com/t342guy/kerosene/releases/tag/1.0.0-a1
