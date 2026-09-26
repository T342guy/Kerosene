#!/usr/bin/env bash
# Move every Kerosene crate to a new version, together.
#
#   scripts/bump-version.sh 1.0.0-a2
#
# Kerosene is one version: the `kerosene` crate's, which follows SemVer on
# that crate's API (see src/docs/versioning.md). Every other crate carries
# the same number and names its siblings at exactly it, so they are released
# together and never mixed. This changes all of those at once, dates the
# Unreleased section of CHANGELOG.md, and updates Cargo.lock.
#
# It does not commit, tag or publish: those stay deliberate.
set -euo pipefail
cd "$(dirname "$0")/.."

new="${1:-}"
if [[ ! "$new" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
    echo "usage: $0 <major.minor.patch[-pre]>, e.g. 1.0.0-a2 or 1.2.0" >&2
    exit 2
fi
old="$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)"
if [ -z "$old" ]; then
    echo "could not read the current version from Cargo.toml" >&2
    exit 1
fi
if [ "$old" = "$new" ]; then
    echo "already $new" >&2
    exit 1
fi

# The workspace version, and every exact pin on it.
sed -i.bak \
    -e "s/^version = \"$old\"$/version = \"$new\"/" \
    -e "s/version = \"=$old\"/version = \"=$new\"/g" \
    Cargo.toml crates/kerosene/Cargo.toml
rm -f Cargo.toml.bak crates/kerosene/Cargo.toml.bak

# The places the docs show a game what to write in its Cargo.toml.
for doc in crates/kerosene/src/lib.rs src/gamedev/making-a-game.md; do
    sed -i.bak -e "s/kerosene = \"$old\"/kerosene = \"$new\"/" \
        -e "s/version = \"$old\", default-features/version = \"$new\", default-features/" "$doc"
    rm -f "$doc.bak"
done

# The changelog: what was Unreleased is now this version, today.
if grep -q '^## \[Unreleased\]' CHANGELOG.md; then
    today="$(date +%Y-%m-%d)"
    sed -i.bak "s/^## \[Unreleased\]$/## [Unreleased]\n\n## [$new] - $today/" CHANGELOG.md
    rm -f CHANGELOG.md.bak
fi

cargo update --workspace --quiet
echo "Kerosene $old -> $new"
echo "Next: review the diff, run the tests, commit, and tag $new."
