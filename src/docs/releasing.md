# Releasing

How a version of Kerosene goes out. [Versioning](versioning.md) says which
number it gets; this is the order of things.

## Before

1. **Choose the number.** Read the changelog's Unreleased section against
   [what the promise covers](versioning.md#what-the-promise-covers). Anything
   marked **Breaking** is a major version (or the next pre-release, before
   `1.0.0`). CI's `semver` job says what `cargo semver-checks` found against
   the last release; it is advice before `1.0.0` and a gate after.
2. **Base content.** If anything under `content/` that the engine's base
   content holds has changed, run `scripts/build-content.sh`, which repacks
   `crates/kerosene-engine/base/base.vault`.
   `cargo test -p kerosene-engine base` fails when it is stale.
3. **Green.** `cargo fmt --check`, `cargo clippy --workspace --all-targets
   -- -D warnings` and `cargo test --workspace`, and CI green on every job,
   including the new-game job on all three platforms.

## The release

```sh
scripts/bump-version.sh 1.0.0-a2
git diff                          # Cargo.toml, CHANGELOG.md, Cargo.lock
git commit -am "Kerosene 1.0.0-a2"
git tag 1.0.0-a2
git push && git push --tags
```

Pushing the tag runs `.github/workflows/release.yml`. It builds
`kerosene-tools` and the stock `kerosene` runtime for Linux, Windows and
macOS, and attaches them to a GitHub release. The release's text is the
changelog's section for the version. A version with a `-` in it is marked
a pre-release.

## crates.io

Publishing is done by hand, because it cannot be undone.

```sh
cargo publish --workspace --dry-run
cargo publish --workspace
```

`--workspace` publishes every crate in dependency order. CI's `package` job
packages them all on every push and checks each is under crates.io's 10 MB
limit, so the dry run should hold no surprises.

**The licence expression** is
`GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0`, in every
crate's `Cargo.toml` and every source file's `SPDX-License-Identifier`
line. `AdditionRef-` is how SPDX (from 2.3) names an addition to a licence
that is not on its list, which the Kerosene Exception is. `cargo-deny`
parses it and `cargo package` carries it as written; `deny.toml` allows
Kerosene's crates under exactly that expression, never the bare GPL. The
first real publish is where crates.io's own check is seen, so read its
answer to the first crate before publishing the rest.

## After

- Check the GitHub release has three archives, and that each one runs:
  `kerosene-tools new` then `cargo play` against the released version.
- Start the next Unreleased section of the changelog as changes land.
