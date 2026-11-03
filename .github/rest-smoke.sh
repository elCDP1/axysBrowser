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
#     Measured with one internal page: 197 MiB with GTK's GL renderer on WSLg
#     and ~269 MiB under Xvfb, against 93 MiB and ~134 MiB with cairo. The
#     bound is 160 MiB: comfortably above every cairo figure (runner variance
#     included) and well below the old software-GL one, so falling back to
#     software GL fails the build instead of silently doubling idle memory.
set -uo pipefail

BOUND_KB=$((160 * 1024))

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

# The app logs its decision together with the evidence for it, e.g.
#   Interface renderer: cairo (software GL: faux_driver on renderD128).
#   Interface renderer: gl (hardware GL: amdgpu on renderD128).
# The evidence is recomputed here, independently of the app, so a wrong
# verdict inside the app cannot make this smoke pass.
DECISION=$(grep -m1 -o "Interface renderer: [a-z]*" "$LOG" | awk '{print $3}')

software_gl=0
DRIVER=""

case "${LIBGL_ALWAYS_SOFTWARE:-}" in
  "" | 0 | false | FALSE | no | off) ;;
  *) software_gl=1 ;;
esac

case "${GALLIUM_DRIVER:-${MESA_LOADER_DRIVER_OVERRIDE:-}}" in
  llvmpipe | softpipe | swrast | swiftshader) software_gl=1 ;;
esac

# A DRI node is not proof of a 3D engine: WSLg exposes renderD128 backed by
# faux_driver, and vgem/vkms are virtual devices. Only a real driver counts.
if [ -z "$(ls /dev/dri/renderD* 2>/dev/null)" ]; then
  software_gl=1
else
  for node in /dev/dri/renderD*; do
    driver=$(basename "$(readlink -f "/sys/class/drm/$(basename "$node")/device/driver" 2>/dev/null)")
    if [ -z "$driver" ]; then
      software_gl=1
    fi
    case "$driver" in
      faux_driver | vgem | vkms) software_gl=1 ;;
    esac
    DRIVER="${DRIVER:+$DRIVER,}$driver"
  done
fi

if [ -n "${GSK_RENDERER:-}" ] || [ -n "${AXYS_GSK:-}" ]; then
  echo "renderer forced by the environment ($DECISION): the choice is not asserted"
elif [ "$software_gl" = 1 ]; then
  if [ "$DECISION" != cairo ]; then
    echo "GL IS SOFTWARE (${DRIVER:-no render node}) BUT THE INTERFACE RENDERER IS '${DECISION:-none}'"
    cat "$LOG"
    exit 1
  fi
  echo "interface renderer: cairo (software GL: ${DRIVER:-no render node})"
else
  if [ "$DECISION" != gl ]; then
    echo "HARDWARE GL IS AVAILABLE (${DRIVER}) BUT THE INTERFACE RENDERER IS '${DECISION:-none}'"
    cat "$LOG"
    exit 1
  fi
  echo "interface renderer: gl (hardware GL: ${DRIVER})"
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

# Report the measurement's own status: CI fails on it, so it must not depend
# on how the EXIT trap above unwinds.
RESULT=$?
trap - EXIT
cleanup
exit "$RESULT"
