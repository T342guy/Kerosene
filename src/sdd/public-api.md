# Public API

## The boundary

`crates/kerosene/src/lib.rs` is the game crate and the API boundary. Its public
surface is what Kerosene's version number follows.

| Part | Contents | SemVer |
|---|---|---|
| Root | `launch`, `LaunchOptions`, `Game`, `Engine`, `EngineConfig`, `VERSION`, `prelude` | covered |
| Stable modules | `engine`, `entity`, `math`, `console`, `physics`, `script`, `ui`, `platform`, `vfs`, `game` | covered |
| `tools` (feature) | the tool crates, for a game that ships an editor | covered |
| `internals` | `asset`, `audio`, `bsp`, `config`, `kv`, `map`, `render`, `walk`, … | **not** covered |
| Re-exported third parties | `egui`, `glam`, `rhai`, `winit`, `serde_json`, `anyhow`, `log` | follows the upstream |

Third-party crates a game names in its own signatures are re-exported so a game
cannot link two versions of `glam` or `egui`.

The crate is `#![warn(missing_docs)]`, and CI builds its docs with warnings
denied.

## Versioning policy

- One lockstep version across all crates; internal dependencies are pinned with
  `=` and moved together by `scripts/bump-version.sh`.
- Bare-version git tags.
- Pre-release (`1.0.0-aN`) versions carry no compatibility promise. Breaking
  changes are recorded in `CHANGELOG.md` under *Changed* and need no migration
  support until 1.0 or until real games exist.
- After 1.0, a change to the covered surface follows SemVer.

Full policy: [Versioning](../docs/versioning.md) and [Releasing](../docs/releasing.md).

## Extension points

| To… | Do |
|---|---|
| Add rules | implement `Game` |
| Add entity classes | register in `Game::classes`; describe in `Game::schema` |
| Add console commands and convars | register in `Game::setup` |
| Handle custom entity or console verbs | `Game::entity_request` / `console_request` |
| Draw HUD or menus | `Game::wants_ui` and `Game::ui`, or the `.kui` UI |
| Persist game state | `Game::save` / `Game::load` |
| React to the store | `Game::platform_event` |
| Reuse the stock game | `kerosene::game::Stock` |

## See also

- [Making a game](../gamedev/making-a-game.md)
- [Versioning](../docs/versioning.md)
- [Crate map](../devnotes/crate-map.md)
