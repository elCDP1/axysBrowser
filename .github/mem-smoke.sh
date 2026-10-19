#!/bin/bash
# CI memory smoke. Runs the browser's built-in AXYS_MEM_SMOKE self-test
# against a LOCAL page (no internet dependence) and checks, from facts the
# self-test reports, that closing tabs really releases WebKit resources:
#
#   * every page actually loaded before being measured (the page sets its
#     title to a marker after allocating memory);
#   * the web processes opened for the tabs are GONE after closing them
#     (deterministic: no memory threshold involved) — in several repeated
#     open/close cycles AND for the last tab of the window, whose process
#     must really exit;
#   * memory is PSS summed over the main process and all descendants, and
#     what the pages added is mostly given back: the residue after closing
#     must stay below half of the growth the pages caused (a ratio of the
#     measured growth, not an absolute size).
#
# Usage: mem-smoke.sh [strict]
#   default: WebKit sandbox off (CI runners cannot guarantee user
#     namespaces); strict: sandbox left on.
set -uo pipefail

if [ "${1:-}" = "strict" ]; then
  echo "strict mode: sandbox enabled"
else
  export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1
fi
export WEBKIT_DISABLE_DMABUF_RENDERER=1

# GApplication single-instance would forward this launch into a running
# browser (which then exits at once and reports nothing): refuse instead
# of producing a confusing false negative.
if pgrep -f "target/debug/axysbrowser" > /dev/null; then
  echo "A browser instance is already running: close it first,"
  echo "a second launch would just activate that window and exit."
  exit 2
fi

WORK=$(mktemp -d)
LOG="$WORK/browser.log"
SERVER_PID=""
cleanup() {
  [ -n "$SERVER_PID" ] && kill "$SERVER_PID" 2>/dev/null
  rm -rf "$WORK"
}
trap cleanup EXIT

# Isolated profile so the smoke never touches real user data.
export XDG_CONFIG_HOME="$WORK/config" XDG_DATA_HOME="$WORK/data" XDG_CACHE_HOME="$WORK/cache"

cat > "$WORK/mem.html" <<'HTML'
<!doctype html><meta charset="utf-8"><title>loading</title>
<script>
  const keep = [];
  for (let i = 0; i < 8; i++) { const b = new Uint8Array(4 * 1024 * 1024); b.fill(i + 1); keep.push(b); }
  window.__keep = keep;
  document.title = "axys-smoke-ready";
</script>
<p>axys memory smoke page</p>
HTML

PORT=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')
python3 -m http.server "$PORT" --bind 127.0.0.1 --directory "$WORK" >/dev/null 2>&1 &
SERVER_PID=$!
sleep 1

export AXYS_MEM_SMOKE=1
export AXYS_MEM_SMOKE_URL="http://127.0.0.1:$PORT/mem.html"

# External timeout is a backstop above the self-test's own worst-case
# budget (~320s: 3 cycles + last-tab + window-close phases); it must be
# higher so a slow run still reports MEM_SMOKE_ERROR/…_DONE instead of
# being SIGTERMed mid-phase.
timeout -s TERM 420 ./target/debug/axysbrowser > "$LOG" 2>&1
STATUS=$?

grep -a "MEM_SMOKE" "$LOG"

if ! grep -aq "MEM_SMOKE_DONE" "$LOG"; then
  echo "MEM SMOKE DID NOT FINISH (exit status $STATUS)"
  tail -n 40 "$LOG"
  exit 1
fi

python3 - "$LOG" <<'PY'
import re, sys

lines = [l for l in open(sys.argv[1], errors="replace") if "MEM_SMOKE_" in l]

def fields(line):
    return {k: int(v) for k, v in re.findall(r"(\w+)=(\d+)", line)}

cycles = [fields(l) for l in lines if "MEM_SMOKE_CYCLE" in l]
last = [fields(l) for l in lines if "MEM_SMOKE_LASTTAB" in l]
window = [fields(l) for l in lines if "MEM_SMOKE_WINDOW" in l]
errors = [l for l in lines if "MEM_SMOKE_ERROR" in l]

problems = [e.strip() for e in errors]

if len(cycles) < 3:
    problems.append(f"expected at least 3 open/close cycles, got {len(cycles)}")

for c in cycles:
    n = c["cycle"]
    if c["loaded"] != c["opened"]:
        problems.append(f"cycle {n}: only {c['loaded']}/{c['opened']} pages loaded - nothing valid to measure")
        continue
    if c["peak_kb"] <= c["base_kb"]:
        problems.append(f"cycle {n}: memory did not grow with {c['opened']} loaded pages")
        continue
    if c["views_freed"] != c["opened"]:
        problems.append(f"cycle {n}: {c['opened'] - c['views_freed']} closed WebView(s) were never freed")

    grown = c["peak_kb"] - c["base_kb"]
    residue = c["after_kb"] - c["base_kb"]
    print(f"cycle {n}: grew {grown} KiB, residue {residue} KiB ({100 * residue / grown:.0f}% of growth), "
          f"web processes {c['web_base']} -> {c['web_peak']} -> {c['web_after']}")

    # Closing the tabs must end their web processes (each tab owns one):
    # deterministic, no memory threshold involved, so it applies to every
    # cycle. The memory residue is only judged from cycle 2 on, because
    # cycle 1 also pays one-off warm-up costs (shared caches, libraries).
    if c["web_after"] > c["web_base"]:
        problems.append(f"cycle {n}: {c['web_after'] - c['web_base']} web process(es) outlived their closed tabs")
    if n >= 2 and residue * 2 > grown:
        problems.append(f"cycle {n}: residue {residue} KiB is more than half of the {grown} KiB the pages added")

if len(last) != 1:
    problems.append("last-tab phase did not report")
else:
    t = last[0]
    if t["loaded"] != 1:
        problems.append("last tab: page did not load before closing")
    if t["tracked"] != 1 or t["view_freed"] != 1:
        problems.append("last tab: the closed tab's WebView was not freed (still referenced)")
    if t["still_alive"] != 0:
        problems.append(f"last tab: {t['still_alive']} of its {t['old_web_processes']} web process(es) still alive after closing it")
    if t["tabs_left"] != 1:
        problems.append(f"last tab: expected 1 fresh tab afterwards, found {t['tabs_left']}")
    print(f"last tab: {t['before_kb']} -> {t['after_kb']} KiB, WebView freed: {t['view_freed']}/{t['tracked']}")

if len(window) != 1:
    problems.append("window-close phase did not report")
else:
    w = window[0]
    if w["loaded"] != 2:
        problems.append("window close: the pages did not load before the window closed")
    if w["views_freed"] != w["tracked"]:
        problems.append(
            f"window close: {w['tracked'] - w['views_freed']} of {w['tracked']} WebView(s) "
            "were never freed after closing the window (reference cycle)")
    print(f"window close: {w['views_freed']}/{w['tracked']} WebViews freed")

if problems:
    print("MEM SMOKE FAILED")
    for p in problems:
        print(" -", p)
    sys.exit(1)

print("MEM SMOKE OK")
PY
