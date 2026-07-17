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

sed -i '' "s/^version = \".*\"/version = \"$new_version\"/" Cargo.toml
sed -i '' "s/\"version\": \".*\"/\"version\": \"$new_version\"/" vscode-extension/package.json
sed -i '' "s/\"version\": \".*\"/\"version\": \"$new_version\"/" jetbrains-plugin/src/main/resources/textmate/pojoc/package.json
sed -i '' "s/^pluginVersion=.*/pluginVersion=$new_version/" jetbrains-plugin/gradle.properties

# Refresh Cargo.lock so it doesn't go stale against the new workspace version.
cargo check --workspace --quiet

echo "bumped to $new_version"
