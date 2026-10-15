#!/bin/bash
# CI GUI smoke test: launch the browser under Xvfb, assert it stays alive
# with no critical errors.
#
# Usage: gui-smoke.sh [strict]
#   default: website sandboxing is off (CI runners cannot guarantee user
#     namespaces; this smoke covers startup and rendering, not the sandbox).
#   strict: sandbox left enabled; catches sandbox-specific breakage on
#     runners where user namespaces do work.
set -u
if [ "${1:-}" = "strict" ]; then
  echo "strict mode: sandbox enabled"
else
  export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1
fi
export WEBKIT_DISABLE_DMABUF_RENDERER=1
LOG=/tmp/axys_gui_smoke.log
rm -f "$LOG"
timeout -s TERM 25 ./target/debug/axysbrowser > "$LOG" 2>&1 &
PID=$!
sleep 18
if ! kill -0 $PID 2>/dev/null; then
  echo "BROWSER DIED DURING SMOKE"
  cat "$LOG"
  exit 1
fi
if grep -q "Gtk-CRITICAL" "$LOG"; then
  echo "CRITICAL GTK ERRORS DURING SMOKE"
  cat "$LOG"
  kill -TERM $PID 2>/dev/null
  exit 1
fi
kill -TERM $PID 2>/dev/null
sleep 1
kill -9 $PID 2>/dev/null
echo "GUI SMOKE OK"
