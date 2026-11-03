#!/bin/bash
# CI rest-memory smoke: with only an internal page (axys://resources) open,
# assert the browser is cheap while idle, from facts observable outside the
# app:
#
#   * where GL is software (a CI runner has no /dev/dri), the window chrome
#     really picked cairo: the startup line says so;
#   * no web process runs for a native internal page (there is no web content
#     to render, so none may be left over);
#   * the whole tree (main process + WebKit helpers) stays under a PSS bound.
#     Measured on WSLg with one internal page: 197 MiB with GTK's GL renderer
#     versus 93 MiB with cairo. The bound sits between both, so falling back
#     to software GL fails the build instead of silently doubling idle memory.
set -uo pipefail

BOUND_KB=$((200 * 1024))

# Same single-instance trap as the other smokes: a running browser would
# absorb this launch (activate + exit at once) and look like a startup crash.
if pgrep -x axysbrowser > /dev/null; then
  echo "A browser instance is already running: close it first."
  exit 2
fi

WORK=$(mktemp -d)
LOG="$WORK/browser.log"
PID=""
cleanup() {
  # The browser runs in its own process group (setsid below): kill the whole
  # group, or a failed run would leave it and its helpers orphaned.
  if [ -n "$PID" ]; then
    kill -TERM -- "-$PID" 2>/dev/null
    sleep 1
    kill -KILL -- "-$PID" 2>/dev/null
  fi
  rm -rf "$WORK"
}
trap cleanup EXIT

# Isolated profile so the smoke never touches real user data.
export XDG_CONFIG_HOME="$WORK/config" XDG_DATA_HOME="$WORK/data" XDG_CACHE_HOME="$WORK/cache"
export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1
export WEBKIT_DISABLE_DMABUF_RENDERER=1

setsid ./target/debug/axysbrowser axys://resources > "$LOG" 2>&1 &
PID=$!

sleep 2
if ! kill -0 "$PID" 2>/dev/null; then
  echo "BROWSER DIED AT STARTUP"
  cat "$LOG"
  exit 1
fi

# Let the window map and the profile loads settle before reading memory.
sleep 8
if ! kill -0 "$PID" 2>/dev/null; then
  echo "BROWSER DIED WHILE IDLE"
  cat "$LOG"
  exit 1
fi

if grep -q "Gtk-CRITICAL" "$LOG"; then
  echo "CRITICAL GTK ERRORS AT REST"
  cat "$LOG"
  exit 1
fi

# On a machine with a real GPU either renderer is legitimate, so the choice is
# only asserted where GL is software.
if [ ! -e /dev/dri ]; then
  if ! grep -q "Interface renderer: cairo" "$LOG"; then
    echo "GL IS SOFTWARE (no /dev/dri) BUT THE INTERFACE RENDERER WAS NOT CAIRO"
    cat "$LOG"
    exit 1
  fi
  echo "interface renderer: cairo (no GPU device present)"
else
  echo "GPU device present: the renderer choice is not asserted"
fi

python3 - "$PID" "$BOUND_KB" <<'PY'
import os
import sys

root = int(sys.argv[1])
bound = int(sys.argv[2])


def name_of(pid):
    try:
        with open(f"/proc/{pid}/comm") as handle:
            return handle.read().strip()
    except OSError:
        return "?"


def pss_kb(pid):
    try:
        with open(f"/proc/{pid}/smaps_rollup") as handle:
            for line in handle:
                if line.startswith("Pss:"):
                    return int(line.split()[1])
    except (OSError, ValueError):
        return 0
    return 0


def children_of(pid):
    found = []
    for entry in os.listdir("/proc"):
        if not entry.isdigit():
            continue
        try:
            with open(f"/proc/{entry}/stat") as handle:
                # The command name is parenthesised and may contain spaces.
                fields = handle.read().rsplit(")", 1)[1].split()
            if int(fields[1]) == pid:
                found.append(int(entry))
        except (OSError, IndexError, ValueError):
            continue
    return found


tree, queue = {}, [root]
while queue:
    pid = queue.pop()
    if pid in tree:
        continue
    tree[pid] = name_of(pid)
    queue.extend(children_of(pid))

for pid, label in sorted(tree.items()):
    print(f"  {pid:>7} {label:<20} {pss_kb(pid) / 1024:>7.1f} MiB PSS")

total = sum(pss_kb(pid) for pid in tree)
print(
    f"total at rest: {total / 1024:.1f} MiB PSS over {len(tree)} process(es) "
    f"(bound {bound / 1024:.0f} MiB)"
)

problems = []

web = [pid for pid, label in tree.items() if label.startswith("WebKitWebProc")]
if web:
    problems.append(f"{len(web)} web process(es) idle on a native internal page")

if total > bound:
    problems.append(
        f"idle memory {total / 1024:.1f} MiB exceeds the {bound / 1024:.0f} MiB bound"
    )

if problems:
    print("REST SMOKE FAILED")
    for problem in problems:
        print(" -", problem)
    sys.exit(1)

print("REST SMOKE OK")
PY
