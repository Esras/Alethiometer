#!/usr/bin/env bash
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && git rev-parse --show-toplevel)
base=$("$here/build-flake-image.sh" "$root" | tail -1)
ctx=$(mktemp -d)
trap 'rm -rf "$ctx"' EXIT
git -C "$root" archive HEAD | tar -x -C "$ctx"
podman build --build-arg "BASE=$base" -t "${1:-localhost/aleth-dev:local}" -f "$here/Dockerfile.alethiometer" "$ctx"
