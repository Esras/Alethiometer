#!/usr/bin/env bash
# Pixel-diff the UI against a git ref, for changes that shouldn't change what
# you see (app/docs/ui-plan.md). Starts two copies side by side:
#   :1430  the UI at <ref> (a worktree), on a dev server at :1421
#   :1420  this checkout's UI,          on a dev server at :1422
# Each dev server gets fresh sample data, the clock at 10:52 and two scripted
# agents (scripts/fake-acp.mjs: one waiting on an approval, one on a question).
# Both use this checkout's dev server binary: only the UI is compared.
# Then: node scripts/ui-diff.mjs [scene…]
#
# usage: scripts/ui-diff.sh [ref]     (default HEAD; "stop" stops everything)
set -euo pipefail
APP=$(cd "$(dirname "$0")/.." && pwd)
WORK=${UI_DIFF_DIR:-${TMPDIR:-/tmp}/alethiometer-ui-diff}
BIN="$APP/src-tauri/target/debug/devserver"

stop() {
  pkill -f "[t]arget/debug/devserver" 2>/dev/null || true
  pkill -f "[a]lethiometer-agentd" 2>/dev/null || true
  pkill -f "[v]ite --port 1430" 2>/dev/null || true
  pkill -f "[v]ite --port 1420" 2>/dev/null || true
  sleep 1
}

if [ "${1:-}" = stop ]; then
  stop
  exit 0
fi
REF=${1:-HEAD}
stop
mkdir -p "$WORK"

# The ref's UI, sharing this checkout's node_modules.
if [ -e "$WORK/base" ]; then git -C "$APP" worktree remove --force "$WORK/base"; fi
git -C "$APP" worktree add --quiet --detach "$WORK/base" "$REF"
ln -s "$APP/node_modules" "$WORK/base/app/node_modules"
# node_modules is a symlink out of the worktree; let Vite serve its fonts.
cat > "$WORK/base/app/vite.ui-diff.config.ts" <<'TS'
import { mergeConfig } from 'vite';
import base from './vite.config';
export default mergeConfig(base, { server: { fs: { strict: false } } });
TS

if [ ! -x "$BIN" ]; then
  (cd "$APP" && npm run agentd > /dev/null && cargo build -q --manifest-path src-tauri/Cargo.toml --no-default-features --bin devserver)
fi

rm -rf "$WORK/data-a" "$WORK/data-b"
mkdir -p "$WORK/proj"
for d in data-a data-b; do
  mkdir -p "$WORK/$d"
  cat > "$WORK/$d/settings.json" <<JSON
{ "agents": { "harnesses": [
  { "key": "fake", "label": "Claude Code", "command": "node", "args": ["$APP/scripts/fake-acp.mjs"], "env": { "FAKE_ACP_FAST": "1" } },
  { "key": "fakeask", "label": "Pi", "command": "node", "args": ["$APP/scripts/fake-acp.mjs"], "env": { "FAKE_ACP_FAST": "1", "FAKE_ACP_MODE": "ask" } }
] } }
JSON
done

export ALETHIOMETER_FAKE_NOW=10:52
# Detached (own session, no inherited stdin/stdout), so they outlive this script.
ALETHIOMETER_DATA_DIR="$WORK/data-a" ALETHIOMETER_DEVSERVER_ADDR=127.0.0.1:1421 setsid -f "$BIN" < /dev/null > "$WORK/dev-a.log" 2>&1
ALETHIOMETER_DATA_DIR="$WORK/data-b" ALETHIOMETER_DEVSERVER_ADDR=127.0.0.1:1422 setsid -f "$BIN" < /dev/null > "$WORK/dev-b.log" 2>&1
(cd "$WORK/base/app" && ALETHIOMETER_DEVSERVER=http://127.0.0.1:1421 setsid -f npx vite --port 1430 --strictPort --config vite.ui-diff.config.ts < /dev/null > "$WORK/vite-a.log" 2>&1)
(cd "$APP" && ALETHIOMETER_DEVSERVER=http://127.0.0.1:1422 setsid -f npx vite --port 1420 --strictPort < /dev/null > "$WORK/vite-b.log" 2>&1)

for _ in $(seq 1 90); do
  if grep -q "devserver on" "$WORK/dev-a.log" 2>/dev/null && grep -q "devserver on" "$WORK/dev-b.log" 2>/dev/null &&
    curl -sf -o /dev/null localhost:1430 && curl -sf -o /dev/null localhost:1420; then
    break
  fi
  sleep 1
done

# The same two agents on both sides, one after the other: lists sort by when an
# agent started asking, so a race would order them differently on each side.
for h in fake fakeask; do
  for port in 1421 1422; do
    curl -sf -o /dev/null -X POST -H 'X-Alethiometer: 1' "http://127.0.0.1:$port/call/agents.start_local" \
      -d "{\"harness\":\"$h\",\"cwd\":\"$WORK/proj\",\"name\":\"$h-agent\",\"task\":\"Fix the retry storm\"}"
  done
  sleep 6
done
echo "ready: $REF on :1430, this checkout on :1420 (logs in $WORK). Next: node scripts/ui-diff.mjs"
