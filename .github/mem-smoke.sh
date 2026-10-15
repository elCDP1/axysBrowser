#!/bin/bash
# CI memory smoke: runs the browser's built-in AXYS_MEM_SMOKE self-test
# (open real tabs, close them, report WebKit RSS) and asserts closing
# frees the page memory again (10% tolerance for shared caches).
set -u
export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export AXYS_MEM_SMOKE=1
LOG=/tmp/axys_mem_smoke.log
rm -f "$LOG"
timeout -s TERM 90 ./target/debug/axysbrowser > "$LOG" 2>&1
LINE=$(grep -a "MEM_SMOKE" "$LOG" | tail -n 1)
echo "$LINE"
if [ -z "$LINE" ]; then
  echo "MEM SMOKE DID NOT REPORT"
  cat "$LOG"
  exit 1
fi
python3 - "$LINE" <<'EOF'
import re, sys
line = sys.argv[1]
m = re.search(r"baseline_kb=(\d+) peak_kb=(\d+) after_kb=(\d+)", line)
assert m, "unparseable MEM_SMOKE line"
base, peak, after = (int(m.group(i)) for i in (1, 2, 3))
print(f"baseline={base} peak={peak} after={after} (KiB)")
assert peak >= base, "peak below baseline: pages never loaded?"
assert after <= peak * 1.10, f"memory not released after closing tabs: {after} > {peak}"
print("MEM SMOKE OK")
EOF
