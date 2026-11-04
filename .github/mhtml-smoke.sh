#!/bin/bash
# CI MHTML export smoke: saves a LOCAL page with the same WebKit call the
# "Save page as…" menu uses and checks the file, not the app's word for it:
#
#   * the page really loaded (its own script sets a title marker);
#   * a .mhtml file is written and it is a multipart/related document;
#   * the HTML part carries the page's marker text and the stylesheet rule;
#   * the image and the stylesheet were embedded, not left as links: their
#     parts are present with payload bytes, and the load requested them;
#   * nothing 404'd while the page loaded.
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
<!doctype html><meta charset="utf-8"><title>loading</title>
<link rel="stylesheet" href="style.css">
<p class="mhtml-marker">AXYS-MHTML-MARKER-7f3a</p>
<img src="pixel.png" width="8" height="8" alt="marker">
<div class="mhtml-background">background</div>
<script>
  // window.load fires after the stylesheet and the image, which is exactly
  // what MHTML has to embed; setting the title earlier saved a document with
  // nothing to embed.
  window.addEventListener("load", () => { document.title = "axys-mhtml-ready"; });
</script>
HTML

cat > "$WORK/style.css" <<'CSS'
.mhtml-marker { color: rgb(1, 2, 3); font-weight: 700; }
.mhtml-background { background-image: url("pixel.png"); }
CSS

python3 - "$WORK/pixel.png" <<'PY'
import struct
import sys
import zlib

size = 8
rows = b"".join(b"\x00" + bytes([255, 128, 0, 255] * size) for _ in range(size))


def chunk(tag, data):
    checksum = zlib.crc32(tag + data) & 0xFFFFFFFF
    return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", checksum)


png = (
    b"\x89PNG\r\n\x1a\n"
    + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
    + chunk(b"IDAT", zlib.compress(rows))
    + chunk(b"IEND", b"")
)

with open(sys.argv[1], "wb") as out:
    out.write(png)
PY

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

export AXYS_MHTML_SMOKE=1
export AXYS_MHTML_SMOKE_OUT="$WORK/page.mhtml"

setsid timeout -s TERM 180 ./target/debug/axysbrowser "http://127.0.0.1:$PORT/page.html" > "$LOG" 2>&1 &
PID=$!

# The self-test quits the app after saving; wait for its marker, not a timer.
for _ in $(seq 1 120); do
  grep -aq "MHTML_SMOKE_DONE" "$LOG" && break
  kill -0 "$PID" 2>/dev/null || break
  sleep 0.5
done

grep -a "MHTML_SMOKE" "$LOG" || true

if ! grep -aq "MHTML_SMOKE_SAVED" "$LOG"; then
  echo "MHTML EXPORT DID NOT SUCCEED"
  tail -n 40 "$LOG"
  exit 1
fi

# Keep a copy outside the temporary directory when an assertion fails, so a
# CI failure can be inspected instead of guessed at.
cp "$WORK/page.mhtml" /tmp/axys-mhtml-last.mhtml 2>/dev/null || true

if grep -q " 404 " "$SERVER_LOG"; then
  echo "A RESOURCE WAS MISSING WHILE THE PAGE LOADED"
  cat "$SERVER_LOG"
  exit 1
fi

if ! grep -q "GET /pixel.png" "$SERVER_LOG"; then
  echo "THE IMAGE WAS NEVER REQUESTED: nothing was there to embed"
  cat "$SERVER_LOG"
  exit 1
fi

python3 - "$WORK/page.mhtml" <<'PY'
import email
import sys
from email import policy

path = sys.argv[1]
data = open(path, "rb").read()

# Parse the MIME document instead of grepping it: WebKit writes the HTML and
# CSS parts quoted-printable and the image base64, so the bytes on disk are
# not the bytes of the page.
message = email.message_from_bytes(data, policy=policy.default)
parts = list(message.walk())

problems = []


def need(condition, message):
    if not condition:
        problems.append(message)


def body(part):
    payload = part.get_payload(decode=True)
    return payload or b""


html = [part for part in parts if part.get_content_type() == "text/html"]
css = [part for part in parts if part.get_content_type() == "text/css"]
images = [part for part in parts if part.get_content_type() == "image/png"]

need(len(data) > 900, f"file is only {len(data)} bytes")
need(message.is_multipart(), "not a MIME multipart document")
need(
    message.get_content_type() == "multipart/related",
    f"top-level type is {message.get_content_type()}",
)

need(html, "no text/html part")
need(
    html and b"AXYS-MHTML-MARKER-7f3a" in body(html[0]),
    "the page's marker text is missing from the html part",
)
need(css, "the stylesheet was not embedded as text/css")
need(css and b".mhtml-marker" in body(css[0]), "the stylesheet rule is missing")
need(
    css and b"rgb(1, 2, 3)" in body(css[0]),
    "the stylesheet's colour was not embedded",
)
need(images, "the image was not embedded as image/png")
need(
    images and body(images[0]).startswith(b"\x89PNG"),
    "the embedded image is not a PNG payload",
)
need(images and len(body(images[0])) > 20, "the embedded image is empty")

addressed = html + css + images
need(
    addressed and all(part.get("Content-Location") for part in addressed),
    "a part has no Content-Location, so readers cannot resolve the resource",
)

for message in problems:
    print(" -", message)

sys.exit(1 if problems else 0)
PY
RESULT=$?

trap - EXIT
cleanup
exit "$RESULT"
