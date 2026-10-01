# Quality

## Strategy

The architecture is arranged so most behaviour is testable without a window,
GPU or audio device:

- **`Engine` is headless.** Integration tests in `crates/kerosene-engine/tests/`
  (`playthrough`, `gameplay`, `acoustics`, `streaming`, `save`, `ui`,
  `platform`, `extensions`) drive whole maps through ticks and assert on world
  state.
- **Plain-data layers.** `kerosene-entity`, `kerosene-console`, `kerosene-kv`
  and `kerosene-bsp` have unit tests with no engine.
- **GPU-free draw tests.** `scene` and the UI produce draw data that tests
  inspect directly; a software rasteriser covers editor views.
- **Offline platform.** `kerosene-platform` has a stand-in backend, so store
  code is tested without Steam. The real Steam backend is tested separately.
- **Layering as a test.** `cargo xtask layers` fails on a forbidden dependency.

Baseline at the phase-0 audit: 84 suites, 2,273 tests, 0 failures. Counts move;
run the suite for current numbers. Detail: [Testing](../devnotes/testing.md).

## Commands

| Check | Command |
|---|---|
| Build | `cargo build --workspace` |
| Test | `cargo test --workspace --locked` |
| Format | `cargo fmt --all --check` |
| Lint | `cargo clippy --workspace --all-targets --locked -- -D warnings` |
| Layering | `cargo xtask layers` |
| Public docs | `RUSTDOCFLAGS=-D warnings cargo doc -p kerosene --no-deps --features tools --locked` |
| Steam backend | `cargo test -p kerosene-platform --features steam` |
| Publishable crate | `cargo xtask bundle` |
| Book links | `python3 scripts/check-links.py` |
| Book build | `mdbook build` |

## Continuous integration

`.github/workflows/ci.yml` runs on pushes to `MASTER` and on pull requests:

| Job | Checks |
|---|---|
| `check` (Linux) | fmt, clippy, layers, tests, Steam tests, docs |
| `platforms` | builds on Windows and macOS |
| `bundle` | builds, packages and plays the published crate |
| `book` | link check and mdBook build |
| deny | `cargo-deny` using `deny.toml` (licences, advisories) |

`mdbook.yml` deploys the book to GitHub Pages; `release.yml` handles releases.

## Known gaps

- No test drives the real `launch` boot path end to end.
- Rendering correctness is checked by capture/smoke tests rather than image
  comparison against references.
- No networking tests, because there is no networking.

See also [Open issues](open-issues.md).

## See also

- [Testing](../devnotes/testing.md)
- [Releasing](../docs/releasing.md)
