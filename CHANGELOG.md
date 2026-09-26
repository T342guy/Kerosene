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
- The tool crates are published as `kerosene-chisel`, `kerosene-cleave`,
  `kerosene-kiln` and so on. Their libraries keep their short names.
- Every Kerosene crate names its siblings at exactly the same version.
- A launch with no content tree runs on the base content instead of warning,
  and a launch with no map opens the demo map.

### Fixed
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

[Unreleased]: https://github.com/t342guy/kerosene/compare/1.0.0-a1...HEAD
[1.0.0-a1]: https://github.com/t342guy/kerosene/releases/tag/1.0.0-a1
