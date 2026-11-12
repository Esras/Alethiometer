#!/usr/bin/env bash
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
proj=$(cd "${1:-$here/../..}" && git rev-parse --show-toplevel)
name=$(basename "$proj" | tr '[:upper:]' '[:lower:]')
tag=${2:-localhost/aleth-flake-$name:local}
ctx=$(mktemp -d)
trap 'rm -rf "$ctx"' EXIT
git -C "$proj" archive HEAD | tar -x -C "$ctx"
mkdir -p "$ctx/.aleth-sandbox"
cp "$here/openshell/policy_advisor.md" "$ctx/.aleth-sandbox/"
cp "$here/patches/pi-bedrock-no-keepalive.mjs" "$ctx/.aleth-sandbox/"
podman build -t "$tag" -f "$here/Dockerfile.flake" "$ctx"
echo "$tag"
