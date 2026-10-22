"""A static file server that sends the headers a cross-origin isolated page
needs (SharedArrayBuffer, and with it the threaded engine), like web/_headers
does in production.

    python3 scripts/serve.py [port] [directory]
"""
import functools
import http.server
import sys


class Handler(http.server.SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header("Cross-Origin-Opener-Policy", "same-origin")
        self.send_header("Cross-Origin-Embedder-Policy", "require-corp")
        self.send_header("Cache-Control", "no-cache")
        super().end_headers()


Handler.extensions_map[".wasm"] = "application/wasm"
port = int(sys.argv[1]) if len(sys.argv) > 1 else 8647
directory = sys.argv[2] if len(sys.argv) > 2 else "web"
http.server.ThreadingHTTPServer(("", port), functools.partial(Handler, directory=directory)).serve_forever()
