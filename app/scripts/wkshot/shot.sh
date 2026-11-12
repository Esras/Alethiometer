#!/bin/sh
# Screenshot the UI in WebKitGTK where there's no browser or display (OpenShell sandboxes):
#   scripts/wkshot/shot.sh <out.png> <WxH> [js] [ms after js]
# Needs the devserver and a built UI served (`npx vite build && npx vite preview --port 1430`; the dev server's
# per-component styles don't load in this WebKit), and broadwayd running (started here if not). `js` runs as
# an async function body after the page loads; what it returns is printed. See app/docs/dev.md.
set -e
here=$(cd "$(dirname "$0")" && pwd)
gtk=$(dirname "$(dirname "$(command -v broadwayd 2>/dev/null || ls /nix/store/*-gtk+3-3.*/bin/broadwayd | grep -v dev | head -n 1)")")
[ -f /tmp/wkshot-shim.so ] || gcc -shared -fPIC -o /tmp/wkshot-shim.so "$here/shmshim.c" -ldl
[ -x "${CARGO_TARGET_DIR:-$here/target}/debug/wkshot" ] || cargo build --offline --manifest-path "$here/Cargo.toml"
mkdir -p /tmp/shm
export LD_PRELOAD=/tmp/wkshot-shim.so GDK_BACKEND=broadway BROADWAY_DISPLAY=:5
export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1 WEBKIT_DISABLE_COMPOSITING_MODE=1
[ -S "$HOME/.cache/broadway6.socket" ] || { "$gtk/bin/broadwayd" :5 >/tmp/broadwayd.log 2>&1 & sleep 2; }
timeout 90 "${CARGO_TARGET_DIR:-$here/target}/debug/wkshot" "${URL:-http://localhost:1430/}" "$1" "$2" 6000 "${3:-}" "${4:-2500}" 2>&1 \
  | grep -v Fontconfig
