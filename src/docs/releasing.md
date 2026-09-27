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
scripts/bump-version.sh 1.0.0-a3
git diff                          # Cargo.toml, CHANGELOG.md, Cargo.lock
git commit -am "Kerosene 1.0.0-a3"
git push
cargo xtask publish               # checks, publishes, and offers to tag
git push origin 1.0.0-a3
```

Pushing the tag runs `.github/workflows/release.yml`. It builds
`kerosene-tools` and the stock `kerosene` runtime for Linux, Windows and
macOS, and attaches them to a **draft** GitHub release. The release's text
is the changelog's section for the version, and a version with a `-` in it
is marked a pre-release. It is a draft because the repository's releases
are immutable: once published, a release takes no more files, so the
archives go on first and you publish the draft from the Releases page when
they are all there.

## crates.io

Kerosene is published as one crate, `kerosene`, with the whole workspace
folded into it. Publishing is done by hand, because it cannot be undone,
and from the repository root:

```sh
cargo xtask publish --dry-run          # everything but the upload
cargo xtask publish
```

Before it uploads anything, `publish` checks for the usual mistakes:

| Finding | What happens |
|---|---|
| This version is already on crates.io, or older than one that is | Stops. Bump it with `scripts/bump-version.sh`. |
| Uncommitted changes | Warns, lists them, and asks |
| Commits not pushed, or no upstream branch | Warns and asks |
| This version is already tagged, at an older commit | Warns — the bump was probably forgotten — and asks |
| `CHANGELOG.md` has no section for this version | Warns and asks |

A dry run shows every finding and carries on to the bundle and
`cargo publish --dry-run` regardless. After a real publish, it offers to tag
the commit with the version; pushing the tag (`git push origin <version>`)
runs the release workflow. `--yes` answers yes to every question, for a
script; with no terminal and no `--yes`, the answer is no.

By hand, the same is:

```sh
cargo xtask bundle                     # target/bundle/kerosene
cd target/bundle/kerosene
cargo publish --allow-dirty            # the bundle is generated, so git ignores it
```

`cargo xtask bundle` copies every workspace crate into
`target/bundle/kerosene/src/__k/` as a module, rewrites the paths between
them, flattens their features into the facade's (`audio`, `steam`,
`tools`), gathers their dependencies into one manifest, and adds the
`kerosene` and `kerosene-tools` binaries and the licence texts. Nothing in
the workspace is published by itself; every package there says
`publish = false`.

CI's `bundle` job does all of that on every push, and runs `cargo package`,
which builds the packaged crate again on its own, as crates.io will, and
checks it is under the 10 MB limit. The `new-game` job makes a game against
the bundle on all three platforms and plays it. So the dry run should hold
no surprises.

Players and game developers install the toolset from it:

```sh
cargo install kerosene --features tools     # kerosene-tools and kerosene
```

**The licence expression** is
`GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0`, in every
crate's `Cargo.toml` and every source file's `SPDX-License-Identifier`
line. `AdditionRef-` is how SPDX (from 2.3) names an addition to a licence
that is not on its list, which the Kerosene Exception is. `cargo-deny`
parses it and `cargo package` carries it as written; `deny.toml` allows
Kerosene's crates under exactly that expression, never the bare GPL.
crates.io accepted it for `1.0.0-a2`.

## After

- Check the draft release has three archives, publish it, and check each
  one runs:
  `kerosene-tools new` then `cargo play` against the released version.
- Start the next Unreleased section of the changelog as changes land.
