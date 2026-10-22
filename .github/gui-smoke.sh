#!/bin/bash
# CI GUI smoke test: launch the browser under Xvfb with a LOCAL page and
# assert that it really renders it, from facts observable outside the app:
#
#   * the browser stays alive and logs no Gtk-CRITICAL;
#   * a WebKit web process is spawned (the plain start page is served
#     without one, so without a URL this smoke would never exercise the
#     process launch where sandbox / user-namespace failures abort the UI);
#   * the page's own JavaScript ran inside that web process: it calls back
#     to the local server (a beacon request), which only happens after the
#     page was loaded AND executed.
#
# Usage: gui-smoke.sh [strict]
#   default: website sandboxing is off (CI runners cannot guarantee user
#     namespaces; this smoke covers startup and rendering, not the sandbox).
#   strict: sandbox forced on (AXYS_FORCE_SANDBOX=1), so the browser's
#     automatic "no sandbox" fallback cannot hide a broken sandbox setup;
#     the smoke also fails if the browser reports having fallen back.
set -uo pipefail

STRICT=0
if [ "${1:-}" = "strict" ]; then
  STRICT=1
  echo "strict mode: sandbox enabled"
  export AXYS_FORCE_SANDBOX=1
  unset WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS
else
  export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1
fi
export WEBKIT_DISABLE_DMABUF_RENDERER=1

# Same single-instance trap as mem-smoke.sh: a running browser would absorb
# this launch (activate + exit at once) and look like a startup crash.
if pgrep -x axysbrowser > /dev/null; then
  echo "A browser instance is already running: close it first."
  exit 2
fi

WORK=$(mktemp -d)
LOG="$WORK/browser.log"
SERVER_LOG="$WORK/server.log"
SERVER_PID=""
PID=""
cleanup() {
  # The browser runs in its own process group (setsid below): kill the whole
  # group, not only `timeout`, or a failed run would leave the browser and
  # its web processes orphaned.
  if [ -n "$PID" ]; then
    kill -TERM -- "-$PID" 2>/dev/null
    sleep 1
    kill -KILL -- "-$PID" 2>/dev/null
  fi
  [ -n "$SERVER_PID" ] && kill "$SERVER_PID" 2>/dev/null
  rm -rf "$WORK"
}
trap cleanup EXIT

# Isolated profile so the smoke never touches real user data.
export XDG_CONFIG_HOME="$WORK/config" XDG_DATA_HOME="$WORK/data" XDG_CACHE_HOME="$WORK/cache"

cat > "$WORK/gui.html" <<'HTML'
<!doctype html><meta charset="utf-8"><title>axys gui smoke</title>
<p>axys gui smoke page</p>
<script>fetch("/axys-rendered").catch(() => {});</script>
HTML

PORT=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')
python3 -m http.server "$PORT" --bind 127.0.0.1 --directory "$WORK" > "$SERVER_LOG" 2>&1 &
SERVER_PID=$!
sleep 1

# True when a WebKit web process is a descendant of $PID.
web_process_running() {
  ps -e -o pid=,ppid=,comm= | awk -v root="$PID" '
    { parent[$1] = $2; name[$1] = $3 }
    END {
      for (p in parent) {
        x = p
        while (x in parent && x != 0 && x != 1) {
          if (x == root) { if (name[p] ~ /^WebKitWebProc/) found = 1; break }
          x = parent[x]
        }
      }
      exit found ? 0 : 1
    }'
}

fail() {
  echo "$1"
  echo "--- browser log:"
  cat "$LOG"
  echo "--- server log:"
  cat "$SERVER_LOG"
  exit 1
}

setsid timeout -s TERM 60 ./target/debug/axysbrowser "http://127.0.0.1:$PORT/gui.html" > "$LOG" 2>&1 &
PID=$!

WEB_PROCESS=0
RENDERED=0
for _ in $(seq 1 90); do
  sleep 0.5
  kill -0 "$PID" 2>/dev/null || fail "BROWSER DIED DURING SMOKE"
  if [ "$WEB_PROCESS" = 0 ] && web_process_running; then WEB_PROCESS=1; fi
  if grep -q "GET /axys-rendered" "$SERVER_LOG"; then RENDERED=1; break; fi
done

[ "$RENDERED" = 1 ] || fail "PAGE NEVER RENDERED (web process seen: $WEB_PROCESS)"

# The page ran; give the UI a moment to misbehave after the first render.
sleep 3
kill -0 "$PID" 2>/dev/null || fail "BROWSER DIED AFTER RENDERING"
[ "$WEB_PROCESS" = 1 ] || web_process_running || fail "NO WEB PROCESS WAS SPAWNED"

if grep -q "Gtk-CRITICAL" "$LOG"; then
  fail "CRITICAL GTK ERRORS DURING SMOKE"
fi

if [ "$STRICT" = 1 ] && grep -Eq "WebKit sandbox unavailable|sandbox disabled" "$LOG"; then
  fail "STRICT MODE FELL BACK TO NO SANDBOX"
fi

echo "GUI SMOKE OK"
