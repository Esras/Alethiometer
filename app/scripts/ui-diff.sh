#!/usr/bin/env bash
# Pixel-diff the UI against a git ref, for changes that shouldn't change what
# you see (app/docs/ui-plan.md). Starts two copies side by side:
#   the UI at <ref> (a worktree)  on port A, with its own dev server
#   this checkout's UI            on port B, with its own dev server
# Each dev server gets fresh sample data, the clock at 10:52 and three scripted
# agents (scripts/fake-acp.mjs: one waiting on an approval, one on a question, one on
# a Pi-style gate that restates a long command).
# Both use the dev server binary of the main checkout: only the UI is compared.
# Then: node scripts/ui-diff.mjs [scene…]
#
# usage: [UI_DIFF_SHARDS=n] scripts/ui-diff.sh [ref]     (default HEAD; "stop" stops this slot)
# Both sides also get a fake Scion Hub (scripts/fake-scion-hub.mjs, one shared:
# no scene posts to it) as a source, so Scion rooms and the composer's @ menu show.
# UI_DIFF_SLOT=n runs a separate copy (ports and folder of its own), so several
# checkouts or worktrees can diff at once. Slot 0: B 1420, A 1430, servers
# 1421/1422, hub 1423. Slot n: B 1600+10n, A +1, servers +2/+3, hub +4.
#
# UI_DIFF_SHARDS=n (1 to 9; default: one fewer than the cores, at most 2) starts n
# such pairs for the slot, so that ui-diff.mjs can shoot its scenes in n parallel
# chunks. Shard 0 is the pair above, on exactly those ports. Shard s>=1 of slot n:
#   B 2000+100n+10s, A +1, servers +2/+3
# (slot 0, shard 1: 2010-2013; slot 1, shard 2: 2120-2123), a range that no slot's
# own ports reach. The hub is one per slot.
# Each shard has its own sample data (data-a, data-b; data-a-s, data-b-s for shard s),
# logs, agents and Vite cache directory (a config file per shard and side: two Vite
# servers on one cache directory serve blank pages). The ref's worktree ($WORK/base) is
# shared by the shards' A servers and this checkout by their B servers. Shots of all
# shards go to $WORK/out. UI_DIFF_SHARDS=1 is the single pair. `stop` stops every
# shard the slot could have, however many were started. The number started is
# written to $WORK/shards, where ui-diff.mjs reads it.
set -euo pipefail
APP=$(cd "$(dirname "$0")/.." && pwd)
SLOT=${UI_DIFF_SLOT:-0}
# Measured on 4 cores (docs/ui-plan.md): a 3rd shard saves little and the box is then
# saturated, which shows up as timing flakes (a tool call's "0.0s" against "0.1s").
DEFAULT_SHARDS=$(($(nproc 2>/dev/null || echo 2) - 1))
if [ "$DEFAULT_SHARDS" -gt 2 ]; then DEFAULT_SHARDS=2; elif [ "$DEFAULT_SHARDS" -lt 1 ]; then DEFAULT_SHARDS=1; fi
SHARDS=${UI_DIFF_SHARDS:-$DEFAULT_SHARDS}
case "$SHARDS" in [1-9]) ;; *) echo "UI_DIFF_SHARDS must be 1 to 9, not '$SHARDS'" >&2; exit 2 ;; esac
case "$SLOT" in [0-9]) ;; *) echo "UI_DIFF_SLOT must be 0 to 9, not '$SLOT'" >&2; exit 2 ;; esac
# The ports of shard $1 of this slot: B A DA DB.
shard_ports() {
  if [ "$1" != 0 ]; then
    local b=$((2000 + 100 * SLOT + 10 * $1))
    echo "$b $((b + 1)) $((b + 2)) $((b + 3))"
  elif [ "$SLOT" = 0 ]; then
    echo "1420 1430 1421 1422"
  else
    echo "$((1600 + 10 * SLOT)) $((1601 + 10 * SLOT)) $((1602 + 10 * SLOT)) $((1603 + 10 * SLOT))"
  fi
}
if [ "$SLOT" = 0 ]; then HUB=1423; else HUB=$((1604 + 10 * SLOT)); fi
WORK=${UI_DIFF_DIR:-${TMPDIR:-/tmp}/alethiometer-ui-diff}/$SLOT
# The main checkout (this may be a worktree): its build and node_modules are shared.
MAIN=$(dirname "$(git -C "$APP" rev-parse --path-format=absolute --git-common-dir)")/app
BIN="$MAIN/src-tauri/target/debug/devserver"

stop() {
  local ports=("$HUB/tcp") s p
  for s in 0 1 2 3 4 5 6 7 8 9; do
    for p in $(shard_ports "$s"); do ports+=("$p/tcp"); done
  done
  fuser -k -TERM "${ports[@]}" > /dev/null 2>&1 || true
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
# tree (fonts, icons); this config lets it. It also gives the copy its own
# dependency cache: two Vite servers sharing node_modules/.vite re-optimise over
# each other, and the loser serves 504 "Outdated Optimize Dep" (a blank page).
# viteconf <app dir> <config file> <cache dir>
viteconf() {
  cat > "$1/$2" <<TS
import { mergeConfig } from 'vite';
import base from './vite.config';
export default mergeConfig(base, { cacheDir: '$3', server: { fs: { strict: false } } });
TS
}

# The ref's UI, sharing the main checkout's node_modules.
if [ -e "$WORK/base" ]; then git -C "$APP" worktree remove --force "$WORK/base"; fi
git -C "$APP" worktree add --quiet --detach "$WORK/base" "$REF"
ln -s "$MAIN/node_modules" "$WORK/base/app/node_modules"
# This checkout: a worktree has no node_modules of its own.
if [ ! -e "$APP/node_modules" ]; then ln -s "$MAIN/node_modules" "$APP/node_modules"; fi
# Where this checkout's node_modules is a symlink, shard 0's B side needs the config too.
LINKED=0
if [ -L "$APP/node_modules" ]; then LINKED=1; fi

if [ ! -x "$BIN" ]; then
  (cd "$MAIN" && npm run agentd > /dev/null && cargo build -q --manifest-path src-tauri/Cargo.toml --no-default-features --bin devserver)
fi

export ALETHIOMETER_FAKE_NOW=10:52
rm -rf "$WORK"/data-* "$WORK/shards"
mkdir -p "$WORK/proj"
# Detached (own session, no inherited stdin/stdout), so they outlive this script.
setsid -f node "$MAIN/scripts/fake-scion-hub.mjs" "$HUB" < /dev/null > "$WORK/hub.log" 2>&1

# The hub as a Scion source on both sides (any token will do).
call() { curl -sf -o /dev/null -X POST -H 'X-Alethiometer: 1' "http://127.0.0.1:$1/call/$2" -d "$3"; }

# Shard $1: its two dev servers with fresh data, its two Vite servers, the hub source
# and the three agents. Run in the background, one per shard.
start_shard() {
  local s=$1 suf="" conf=vite.ui-diff.config.ts cache=.vite-ui-diff
  local PB PA DA DB
  read -r PB PA DA DB <<< "$(shard_ports "$s")"
  local conf_b=()
  if [ "$s" != 0 ]; then
    suf=-$s
    conf=vite.ui-diff-$SLOT-$s.config.ts
    cache=.vite-ui-diff-$SLOT-$s
  fi
  if [ "$s" != 0 ] || [ "$LINKED" = 1 ]; then
    viteconf "$APP" "$conf" "$cache"
    conf_b=(--config "$conf")
  fi
  viteconf "$WORK/base/app" "$conf" "$cache"

  local d
  for d in data-a data-b; do
    mkdir -p "$WORK/$d$suf"
    cat > "$WORK/$d$suf/settings.json" <<JSON
{ "agents": { "harnesses": [
  { "key": "fake", "label": "Claude Code", "command": "node", "args": ["$APP/scripts/fake-acp.mjs"], "env": { "FAKE_ACP_FAST": "1" } },
  { "key": "fakeask", "label": "Pi", "command": "node", "args": ["$APP/scripts/fake-acp.mjs"], "env": { "FAKE_ACP_FAST": "1", "FAKE_ACP_MODE": "ask" } },
  { "key": "fakepi", "label": "Pi", "command": "node", "args": ["$APP/scripts/fake-acp.mjs"], "env": { "FAKE_ACP_FAST": "1", "FAKE_ACP_MODE": "pigate" } }
] } }
JSON
  done

  ALETHIOMETER_DATA_DIR="$WORK/data-a$suf" ALETHIOMETER_DEVSERVER_ADDR=127.0.0.1:$DA setsid -f "$BIN" < /dev/null > "$WORK/dev-a$suf.log" 2>&1
  ALETHIOMETER_DATA_DIR="$WORK/data-b$suf" ALETHIOMETER_DEVSERVER_ADDR=127.0.0.1:$DB setsid -f "$BIN" < /dev/null > "$WORK/dev-b$suf.log" 2>&1
  (cd "$WORK/base/app" && ALETHIOMETER_DEVSERVER=http://127.0.0.1:$DA setsid -f npx vite --port "$PA" --strictPort --config "$conf" < /dev/null > "$WORK/vite-a$suf.log" 2>&1)
  (cd "$APP" && ALETHIOMETER_DEVSERVER=http://127.0.0.1:$DB setsid -f npx vite --port "$PB" --strictPort "${conf_b[@]}" < /dev/null > "$WORK/vite-b$suf.log" 2>&1)

  for _ in $(seq 1 90); do
    if grep -q "devserver on" "$WORK/dev-a$suf.log" 2>/dev/null && grep -q "devserver on" "$WORK/dev-b$suf.log" 2>/dev/null &&
      curl -sf -o /dev/null "localhost:$PA" && curl -sf -o /dev/null "localhost:$PB"; then
      break
    fi
    sleep 1
  done

  local port
  for port in "$DA" "$DB"; do
    call "$port" settings.patch "{\"edits\":[{\"op\":\"push\",\"path\":[\"sources\"],\"value\":{\"kind\":\"scion\",\"name\":\"hub\",\"tag\":\"work\",\"config\":{\"hub_url\":\"http://127.0.0.1:$HUB\"}}}]}"
    call "$port" settings.secret_set '{"name":"scion.token","value":"ui-diff"}'
  done
  sleep 4

  # The same agents on both sides, one after the other: lists sort by when an
  # agent started asking, so a race would order them differently on each side.
  local h
  for h in fake fakeask fakepi; do
    for port in "$DA" "$DB"; do
      curl -sf -o /dev/null -X POST -H 'X-Alethiometer: 1' "http://127.0.0.1:$port/call/agents.start_local" \
        -d "{\"harness\":\"$h\",\"cwd\":\"$WORK/proj\",\"name\":\"$h-agent\",\"task\":\"Fix the retry storm\"}"
    done
    sleep 6
  done
}

pids=()
for ((s = 0; s < SHARDS; s++)); do
  start_shard "$s" &
  pids+=($!)
done
failed=0
for pid in "${pids[@]}"; do wait "$pid" || failed=1; done
if [ "$failed" = 1 ]; then
  echo "a shard failed to start (logs in $WORK)" >&2
  exit 1
fi
echo "$SHARDS" > "$WORK/shards"
read -r PB PA _ _ <<< "$(shard_ports 0)"
echo "ready (slot $SLOT, $SHARDS shard(s)): $REF on :$PA, this checkout on :$PB (logs in $WORK). Next: UI_DIFF_SLOT=$SLOT node scripts/ui-diff.mjs"
