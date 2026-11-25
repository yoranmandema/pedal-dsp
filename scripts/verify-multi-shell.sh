#!/usr/bin/env bash
# Manual/semiautomatic check: two shells, each with its own engine; closing a
# shell leaves no orphan engine. Uses the headless (silent) engine.
set -u
cd "$(dirname "$0")/.."
source scripts/agent-env.sh >/dev/null 2>&1
T="${CARGO_TARGET_DIR:-target}/debug"
export PEDAL_SHELL_HEADLESS=1 PEDAL_ENGINE="$T/pedal-engine"
engines() { pgrep -f "pedal-engine .*--instance shell-" | wc -l | tr -d ' '; }
"$T/pedal-shell" >/tmp/pedal-verify-s1.log 2>&1 & S1=$!
"$T/pedal-shell" >/tmp/pedal-verify-s2.log 2>&1 & S2=$!
sleep 8
grep -h "^pedal-shell: instance" /tmp/pedal-verify-s1.log /tmp/pedal-verify-s2.log
echo "engines with both shells open: $(engines) (want 2)"
kill -TERM $S1; sleep 4
echo "engines after killing shell 1 (SIGTERM): $(engines) (want 1)"
kill -9 $S2; sleep 3
echo "engines after kill -9 of shell 2:       $(engines) (want 0)"
pkill -f "pedal-engine .*--instance shell-" 2>/dev/null
