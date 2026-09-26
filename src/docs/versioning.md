# Versioning

Kerosene has one version number, and it is the `kerosene` crate's. That
crate is what a game depends on, so its API is what a version number has to
be honest about. It follows [Semantic Versioning](https://semver.org):

| Bump | Means for a game |
|---|---|
| **Major**, `2.0.0` | Something a game could have used changed or went away. The [changelog](https://github.com/t342guy/kerosene/blob/MASTER/CHANGELOG.md) says what to change. |
| **Minor**, `1.3.0` | Something was added. A game that built before still builds, and plays the same. |
| **Patch**, `1.2.1` | Something was fixed. Nothing was added. |

Every other crate in the workspace — the engine, the tools, the formats —
carries the same number, and each names the others at exactly it. They are
released together and are never mixed: `kerosene-engine 1.2.0` with
`kerosene-bsp 1.1.0` is not a thing that exists.

## What the promise covers

**Covered**, where a breaking change needs a major version:

- The items at the root of `kerosene`: `launch`, `LaunchOptions`, `Game`,
  `Engine`, `EngineConfig`, `VERSION`, and `kerosene::prelude`.
- The modules `kerosene::engine`, `entity`, `math`, `console`, `physics`,
  `script`, `ui`, `platform`, `vfs`, `game` and, with the `tools` feature,
  `tools`.
- The third-party crates `kerosene` re-exports for a game's own use: `egui`,
  `glam`, `rhai`, `winit`, `serde_json`, `anyhow` and `log`. A breaking
  release of one of them reaches Kerosene only in a major release.
- The Cargo features `audio`, `steam` and `tools`.
- What a game's content relies on: the entity classes and their keys, inputs
  and outputs; the `.keroproj` keys; the console commands and convars the
  engine registers; the Rhai functions scripts can call; and the command-line
  flags of the engine and the tools. A map that loads and plays in `1.2`
  loads and plays in `1.3`.
- The file formats, as the next section says.
- The minimum Rust version, `rust-version` in `Cargo.toml`. Raising it is a
  **minor** change, never a patch, and is always in the changelog.

**Not covered**, and free to change in a minor release:

- `kerosene::internals`: `asset`, `audio`, `bsp`, `config`, `kv`, `map`,
  `render`, `rigid` and `walk`. They are public because tools and ambitious
  games need them, and they change as the engine does.
- Anything `#[doc(hidden)]`, such as `Engine::level` and `Engine::physics`.
- The engine crates when named directly (`kerosene-engine`,
  `kerosene-bsp`…) rather than through `kerosene`. A game depends on
  `kerosene`, and the promise is made there.
- Log messages, console output wording and the look of the editor.

## How Kerosene keeps it

- **Types that will grow are built to.** `LaunchOptions`, `EngineConfig`
  and `kerosene_tools::Options` are `#[non_exhaustive]` with constructors and
  setters, so a new option is not a breaking change. The same goes for
  enums a game only matches on, such as the error enums, `PlatformAction` and
  `StatKind`.
- **Enums the engine dispatches on are not `#[non_exhaustive]`.**
  `ScriptAction`, `UiAction`, `PlatformEvent`, `WeaponEvent` and the two
  `Value` enums are matched exhaustively inside the engine, so that a new
  variant cannot be forgotten there. The price is that adding a variant to
  one of them is a major change.
- **`Game`'s methods all have defaults**, so a new hook is a minor change.
- **CI checks it.** `cargo semver-checks` compares `kerosene`'s API with the
  last release on every push. During the `1.0.0` pre-releases it reports; from
  `1.0.0` it fails the build.

## Formats

Compiled files and saved games have their own version numbers, apart from
the crate's:

| Format | Magic | Version |
|---|---|---|
| Compiled map | `KROS` | 2, and each lump its own |
| Model | `KRMD` | 2 |
| Texture | `KRTX` | 1 |
| Sound | `KRAU` | 1 |
| Archive | `KVLT` | 1 |
| Acoustics lump | `ACST` | 1 |
| Cubemaps lump | `KCUB` | 1 |
| Walk data | `KRWL` | 1 |
| Map source | `.keromap` | 1 |
| Saved game | `.kerosave` | 1 |

A format change is judged by what it does to files people already have:

- A new version that still reads the old one — a new lump, a new optional
  key — is a **minor** change.
- A new version that cannot read the old one is a **major** change, even if
  no Rust signature moved. A game's players have saved games, and a game's
  developers have compiled maps.

Compiled content is rebuilt from source with `cargo play` or `kiln`, so a
format change is also listed in the changelog with what to rebuild.

## Pre-releases

Until `1.0.0`, versions are `1.0.0-aN` (alpha), then `1.0.0-bN` (beta), then
`1.0.0-rc.N`. A pre-release may break the API from the one before, and the
changelog marks each such change **Breaking** with what to do. Cargo never
picks a pre-release for a game on its own: a game opts in by naming one.

## Making a release

`scripts/bump-version.sh <version>` sets every crate's version and exact
pins, dates the changelog's Unreleased section and updates `Cargo.lock`.
Tags are the bare version, `1.0.0-a2`, and pushing one builds the toolset
and runtime for each platform and attaches them to a GitHub release.
[Releasing](releasing.md) is the whole checklist.
