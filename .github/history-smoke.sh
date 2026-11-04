#!/bin/bash
# CI history persistence smoke: proves that a visit made through the real tab
# pipeline reaches history.toml when the browser exits normally.
#
# History writes are deferred and coalesced, so the file only appears after
# the shutdown flush. Unit tests call the manager directly and cannot see a
# missing flush; this drives the actual process, then inspects the file:
#
#   * the page really loaded (its own script sets a title marker);
#   * the browser exits cleanly after quitting on its own;
#   * history.toml exists, holds exactly one visit with that URL and the
#     page's title, and no second copy from a reload;
#   * the file and its directory are owner-only (0600 / 0700).
set -uo pipefail

# Same single-instance trap as the other smokes: a running browser would
# absorb this launch (activate + exit at once) and look like a startup crash.
if pgrep -x axysbrowser > /dev/null; then
  echo "A browser instance is already running: close it first."
  exit 2
fi

export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1
export WEBKIT_DISABLE_DMABUF_RENDERER=1

WORK=$(mktemp -d)
LOG="$WORK/browser.log"
SERVER_LOG="$WORK/server.log"
SERVER_PID=""
PID=""
cleanup() {
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

cat > "$WORK/page.html" <<'HTML'
<!doctype html><meta charset="utf-8"><title>axys-history-ready</title>
<p>axys history smoke page</p>
HTML

python3 "$(dirname "${BASH_SOURCE[0]}")/smoke-server.py" "$WORK" "$WORK/port" > "$SERVER_LOG" 2>&1 &
SERVER_PID=$!

# The server picks its own port and publishes it: no probe-then-release race.
PORT=""
for _ in $(seq 1 100); do
  if [ -s "$WORK/port" ]; then
    PORT=$(cat "$WORK/port")
    break
  fi
  kill -0 "$SERVER_PID" 2>/dev/null || break
  sleep 0.1
done
if [ -z "$PORT" ]; then
  echo "SMOKE SERVER FAILED TO START"
  cat "$SERVER_LOG"
  exit 1
fi

export AXYS_HISTORY_SMOKE=1

setsid timeout -s TERM 60 ./target/debug/axysbrowser "http://127.0.0.1:$PORT/page.html" > "$LOG" 2>&1 &
PID=$!

# The self-test quits the app after the visit is recorded; wait for its
# marker, not a fixed timer.
for _ in $(seq 1 120); do
  grep -aq "HISTORY_SMOKE_DONE" "$LOG" && break
  kill -0 "$PID" 2>/dev/null || break
  sleep 0.5
done

grep -a "HISTORY_SMOKE" "$LOG" || true

if ! grep -aq "HISTORY_SMOKE_VISITED" "$LOG"; then
  echo "THE PAGE NEVER LOADED"
  tail -n 40 "$LOG"
  exit 1
fi

# `app.quit()` returns from the main loop; the shutdown hook flushes the
# deferred write. Wait for the process instead of assuming it finished.
wait "$PID" 2>/dev/null
EXITED=$?

if [ "$EXITED" -ne 0 ]; then
  echo "BROWSER DID NOT EXIT CLEANLY (status $EXITED)"
  tail -n 40 "$LOG"
  exit 1
fi

HISTORY="$XDG_CONFIG_HOME/axysbrowser/history.toml"

if [ ! -f "$HISTORY" ]; then
  echo "NO HISTORY FILE AFTER A NORMAL EXIT: the shutdown flush did not run"
  ls -la "$XDG_CONFIG_HOME/axysbrowser" 2>/dev/null || true
  tail -n 40 "$LOG"
  exit 1
fi

MODE=$(stat -c '%a' "$HISTORY")
if [ "$MODE" != "600" ]; then
  echo "HISTORY FILE MODE IS $MODE, EXPECTED 600"
  exit 1
fi

DIR_MODE=$(stat -c '%a' "$(dirname "$HISTORY")")
if [ "$DIR_MODE" != "700" ]; then
  echo "CONFIG DIR MODE IS $DIR_MODE, EXPECTED 700"
  exit 1
fi

python3 - "$HISTORY" "http://127.0.0.1:$PORT/page.html" <<'PY'
import sys
import tomllib

path, url = sys.argv[1], sys.argv[2]

with open(path, "rb") as handle:
    store = tomllib.load(handle)

entries = store.get("entries", [])
print(f"history entries on disk: {len(entries)}")

problems = []
matching = [entry for entry in entries if entry.get("url") == url]

if len(entries) != 1:
    problems.append(f"expected one entry, found {[entry.get('url') for entry in entries]}")

if len(matching) != 1:
    problems.append(f"expected exactly one visit for {url}, found {len(matching)}")
else:
    title = matching[0].get("title", "")
    if title != "axys-history-ready":
        problems.append(f"stored title is {title!r}, expected the page's own title")
    if matching[0].get("visited_at", 0) <= 0:
        problems.append("visited_at was not recorded")

for message in problems:
    print(" -", message)

sys.exit(1 if problems else 0)
PY
RESULT=$?

trap - EXIT
cleanup
exit "$RESULT"
