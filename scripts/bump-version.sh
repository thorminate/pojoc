#!/usr/bin/env bash
# Bumps the project version everywhere it's duplicated outside Cargo.toml's
# workspace.package (which every crate inherits via version.workspace = true).
#
# Usage: scripts/bump-version.sh 0.3.0
set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <new-version>" >&2
    exit 1
fi

new_version="$1"
if [[ ! "$new_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "error: expected a semver like 0.3.0, got '$new_version'" >&2
    exit 1
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

# BSD sed requires a separate empty backup extension; GNU sed does not.
if sed --version >/dev/null 2>&1; then
    sed_in_place=(-i)
else
    sed_in_place=(-i '')
fi

sed "${sed_in_place[@]}" "s/^version = \".*\"/version = \"$new_version\"/" Cargo.toml
sed "${sed_in_place[@]}" "s/\"version\": \".*\"/\"version\": \"$new_version\"/" vscode-extension/package.json
sed "${sed_in_place[@]}" "s/\"version\": \".*\"/\"version\": \"$new_version\"/" jetbrains-plugin/src/main/resources/textmate/pojoc/package.json
sed "${sed_in_place[@]}" "s/^pluginVersion=.*/pluginVersion=$new_version/" jetbrains-plugin/gradle.properties

# Published path dependencies also need their registry versions updated.
sed "${sed_in_place[@]}" -E "s/(pojoc-(compiler|build-macros) = \{ path = \"[^\"]+\", version = \"=)[^\"]+/\1$new_version/" Cargo.toml
sed "${sed_in_place[@]}" -E "s/(pojoc-build = \{ path = \"\.\.\/build\", version = \")[^\"]+/\1$new_version/" crates/cli/Cargo.toml

# Update the npm package and its lockfile together.
npm --prefix packages/runtime version "$new_version" --allow-same-version --no-git-tag-version --ignore-scripts

# Refresh Cargo.lock so it doesn't go stale against the new workspace version.
cargo check --workspace --quiet

echo "bumped to $new_version"
