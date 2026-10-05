#!/usr/bin/env bash
# Pixel-diff the UI against a git ref, for changes that shouldn't change what
# you see (app/docs/ui-plan.md). Starts two copies side by side:
#   the UI at <ref> (a worktree)  on port A, with its own dev server
#   this checkout's UI            on port B, with its own dev server
# Each dev server gets fresh sample data, the clock at 10:52 and two scripted
# agents (scripts/fake-acp.mjs: one waiting on an approval, one on a question).
# Both use the dev server binary of the main checkout: only the UI is compared.
# Then: node scripts/ui-diff.mjs [scene…]
#
# usage: scripts/ui-diff.sh [ref]     (default HEAD; "stop" stops this slot)
# UI_DIFF_SLOT=n runs a separate copy (ports and folder of its own), so several
# checkouts or worktrees can diff at once. Slot 0: B 1420, A 1430, servers
# 1421/1422. Slot n: B 1600+10n, A +1, servers +2/+3.
set -euo pipefail
APP=$(cd "$(dirname "$0")/.." && pwd)
SLOT=${UI_DIFF_SLOT:-0}
if [ "$SLOT" = 0 ]; then
  PB=1420 PA=1430 DA=1421 DB=1422
else
  PB=$((1600 + 10 * SLOT)) PA=$((1601 + 10 * SLOT)) DA=$((1602 + 10 * SLOT)) DB=$((1603 + 10 * SLOT))
fi
WORK=${UI_DIFF_DIR:-${TMPDIR:-/tmp}/alethiometer-ui-diff}/$SLOT
# The main checkout (this may be a worktree): its build and node_modules are shared.
MAIN=$(dirname "$(git -C "$APP" rev-parse --path-format=absolute --git-common-dir)")/app
BIN="$MAIN/src-tauri/target/debug/devserver"

stop() {
  for port in "$PA" "$PB" "$DA" "$DB"; do fuser -k -TERM "$port/tcp" > /dev/null 2>&1 || true; done
  pkill -f "agentd --dir $WORK/" 2>/dev/null || true
  sleep 1
}

if [ "${1:-}" = stop ]; then
  stop
  exit 0
fi
REF=${1:-HEAD}
stop
mkdir -p "$WORK"

# Vite refuses to serve files from a node_modules that is a symlink out of the
# tree (fonts, icons); this config lets it.
viteconf() {
  cat > "$1/vite.ui-diff.config.ts" <<'TS'
import { mergeConfig } from 'vite';
import base from './vite.config';
export default mergeConfig(base, { server: { fs: { strict: false } } });
TS
}

# The ref's UI, sharing the main checkout's node_modules.
if [ -e "$WORK/base" ]; then git -C "$APP" worktree remove --force "$WORK/base"; fi
git -C "$APP" worktree add --quiet --detach "$WORK/base" "$REF"
ln -s "$MAIN/node_modules" "$WORK/base/app/node_modules"
viteconf "$WORK/base/app"
# This checkout: a worktree has no node_modules of its own.
CONF_B=()
if [ ! -e "$APP/node_modules" ]; then ln -s "$MAIN/node_modules" "$APP/node_modules"; fi
if [ -L "$APP/node_modules" ]; then
  viteconf "$APP"
  CONF_B=(--config vite.ui-diff.config.ts)
fi

if [ ! -x "$BIN" ]; then
  (cd "$MAIN" && npm run agentd > /dev/null && cargo build -q --manifest-path src-tauri/Cargo.toml --no-default-features --bin devserver)
fi

rm -rf "$WORK/data-a" "$WORK/data-b"
mkdir -p "$WORK/proj"
for d in data-a data-b; do
  mkdir -p "$WORK/$d"
  cat > "$WORK/$d/settings.json" <<JSON
{ "agents": { "harnesses": [
  { "key": "fake", "label": "Claude Code", "command": "node", "args": ["$MAIN/scripts/fake-acp.mjs"], "env": { "FAKE_ACP_FAST": "1" } },
  { "key": "fakeask", "label": "Pi", "command": "node", "args": ["$MAIN/scripts/fake-acp.mjs"], "env": { "FAKE_ACP_FAST": "1", "FAKE_ACP_MODE": "ask" } }
] } }
JSON
done

export ALETHIOMETER_FAKE_NOW=10:52
# Detached (own session, no inherited stdin/stdout), so they outlive this script.
ALETHIOMETER_DATA_DIR="$WORK/data-a" ALETHIOMETER_DEVSERVER_ADDR=127.0.0.1:$DA setsid -f "$BIN" < /dev/null > "$WORK/dev-a.log" 2>&1
ALETHIOMETER_DATA_DIR="$WORK/data-b" ALETHIOMETER_DEVSERVER_ADDR=127.0.0.1:$DB setsid -f "$BIN" < /dev/null > "$WORK/dev-b.log" 2>&1
(cd "$WORK/base/app" && ALETHIOMETER_DEVSERVER=http://127.0.0.1:$DA setsid -f npx vite --port "$PA" --strictPort --config vite.ui-diff.config.ts < /dev/null > "$WORK/vite-a.log" 2>&1)
(cd "$APP" && ALETHIOMETER_DEVSERVER=http://127.0.0.1:$DB setsid -f npx vite --port "$PB" --strictPort "${CONF_B[@]}" < /dev/null > "$WORK/vite-b.log" 2>&1)

for _ in $(seq 1 90); do
  if grep -q "devserver on" "$WORK/dev-a.log" 2>/dev/null && grep -q "devserver on" "$WORK/dev-b.log" 2>/dev/null &&
    curl -sf -o /dev/null "localhost:$PA" && curl -sf -o /dev/null "localhost:$PB"; then
    break
  fi
  sleep 1
done

# The same two agents on both sides, one after the other: lists sort by when an
# agent started asking, so a race would order them differently on each side.
for h in fake fakeask; do
  for port in "$DA" "$DB"; do
    curl -sf -o /dev/null -X POST -H 'X-Alethiometer: 1' "http://127.0.0.1:$port/call/agents.start_local" \
      -d "{\"harness\":\"$h\",\"cwd\":\"$WORK/proj\",\"name\":\"$h-agent\",\"task\":\"Fix the retry storm\"}"
  done
  sleep 6
done
echo "ready (slot $SLOT): $REF on :$PA, this checkout on :$PB (logs in $WORK). Next: UI_DIFF_SLOT=$SLOT node scripts/ui-diff.mjs"
