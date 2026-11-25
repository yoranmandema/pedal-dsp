#!/usr/bin/env bash
# Launch Pedal with an up-to-date UI.
#
#   ./shell/run.sh dev     live-reload loop: Vite + the debug shell.
#                          UI edits appear on save; no rebuild needed.
#   ./shell/run.sh ship    the shipped app: rebuild Pedal.app, then open it.
#
# Each shell spawns and owns its own engine on a free loopback port and injects
# that URL into its own window (window.__PEDAL_ENGINE_URL__), and stops the
# engine when it closes. No port is shared, so you can run this several times
# (or open Pedal.app next to a dev shell): every window drives its own engine.
#
# How fresh is the UI?
#   dev   -> always current. The debug shell loads Vite's devUrl, not the
#            embedded bundle, so `ui/src` edits show up immediately.
#   ship  -> a snapshot. Pedal.app embeds ui/dist at build time, so it only
#            changes when this script rebuilds it.
set -euo pipefail
cd "$(dirname "$0")/.."

mode="${1:-dev}"
# The shell's devUrl is http://localhost:5173 (tauri.conf.json). If another Vite
# is already there, ours silently moves to 5174 and the shell still loads 5173,
# so it would show someone else's session. Clear the port and pin ours to it.
free_vite() {
  { lsof -iTCP:5173 -sTCP:LISTEN -n 2>/dev/null || true; } | tail -n +2 | awk '{print $2}' | sort -u |
    while read -r pid; do
      [ -n "$pid" ] || continue
      echo "    stopping stale Vite pid $pid"
      kill -9 "$pid" 2>/dev/null || true
    done
  sleep 1
}

case "$mode" in
  dev)
    free_vite
    # NOTE: no --features custom-protocol. A plain debug build is a Tauri *dev*
    # build and loads Vite's devUrl; that is exactly what gives live reload.
    cargo build -p pedal-shell
    # Node + pnpm: prefer your own install; fall back to the toolchain Hermes
    # ships in ~/.hermes/tools (which kanban workers use) so this works either way.
    if ! command -v node >/dev/null 2>&1; then
      for d in "$HOME"/.hermes/tools/node-*/bin; do [ -x "$d/node" ] && PATH="$d:$PATH"; done
    fi
    if ! command -v pnpm >/dev/null 2>&1; then
      for d in "$HOME"/.hermes/tools/pnpm-*; do [ -x "$d/pnpm" ] && PATH="$d:$PATH"; done
    fi
    export PATH
    if ! command -v node >/dev/null 2>&1 || ! command -v pnpm >/dev/null 2>&1; then
      echo "✗ node/pnpm not found. Install them with:  brew install node pnpm" >&2
      exit 1
    fi
    [ -d ui/node_modules ] || ( cd ui && pnpm install --frozen-lockfile )
    echo "• starting Vite on http://localhost:5173"
    ( cd ui && pnpm dev --port 5173 --strictPort ) &
    vite=$!
    trap 'kill $vite 2>/dev/null || true' EXIT
    # Don't open the window until Vite answers, or the webview shows a blank page.
    for _ in $(seq 60); do
      lsof -iTCP:5173 -sTCP:LISTEN -n >/dev/null 2>&1 && break
      kill -0 "$vite" 2>/dev/null || { echo "✗ Vite exited — see the error above" >&2; exit 1; }
      sleep 0.5
    done
    lsof -iTCP:5173 -sTCP:LISTEN -n >/dev/null 2>&1 || { echo "✗ Vite did not start within 30 s" >&2; exit 1; }
    echo "• starting the shell (it spawns its own engine on a free port)"
    shell_out="$(mktemp -t pedal-shell.XXXXXX)"
    ./target/debug/pedal-shell > "$shell_out" 2>&1 &
    shell=$!
    echo -n "• waiting for the engine"
    for _ in $(seq 1 60); do
      if grep -q "^pedal-shell: instance" "$shell_out" 2>/dev/null; then
        echo " up: $(grep '^pedal-shell: instance' "$shell_out" | head -1)"
        break
      fi
      if ! kill -0 "$shell" 2>/dev/null; then
        echo
        echo "✗ the shell exited before the engine came up:"
        tail -20 "$shell_out"
        exit 1
      fi
      echo -n "."
      sleep 0.5
    done
    wait "$shell"
    ;;

  ship)
    ./shell/build-bundle.sh
    open target/release/bundle/macos/Pedal.app
    ;;

  *)
    echo "usage: $0 [dev|ship]" >&2
    exit 2
    ;;
esac
