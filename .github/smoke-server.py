#!/usr/bin/env python3
"""Tiny local HTTP server for the smoke tests.

Usage: smoke-server.py DIRECTORY PORT_FILE

Binds an OS-chosen port itself and then publishes it by atomically writing
PORT_FILE, so the callers never probe for a "free" port and release it
before the server starts (a race), and can wait for the file instead of
sleeping. Requests are logged to stderr, one line each.
"""

import functools
import http.server
import os
import sys

directory, port_file = sys.argv[1], sys.argv[2]

handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=directory)
server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)

tmp = port_file + ".tmp"
with open(tmp, "w") as out:
    out.write(str(server.server_address[1]))
os.replace(tmp, port_file)

server.serve_forever()
