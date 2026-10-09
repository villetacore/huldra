"""Test web server for tests/net.txt, started by `cargo xtask test`.

    python3 tests/net/server.py HTTP_PORT HTTPS_PORT CERT KEY [GIT_ROOT]

Serves the same handler over HTTP and HTTPS (TLS 1.3 only):

  /hello.txt        a small text file
  /redirect         302 to /hello.txt (on HTTP: to the HTTPS server)
  /chunked          a chunked response
  /gzip             a gzip-encoded response
  /big              1 MiB of predictable bytes, with Range support
  /echo             POST: echoes the body back
  /page.html        an HTML page with links and a form (for the browser)
  /git/...          repositories under GIT_ROOT through `git http-backend`
                    (smart HTTP, push allowed)
"""
import gzip, http.server, os, socketserver, ssl, subprocess, sys, threading

HTTP_PORT, HTTPS_PORT, CERT, KEY = int(sys.argv[1]), int(sys.argv[2]), sys.argv[3], sys.argv[4]
GIT_ROOT = sys.argv[5] if len(sys.argv) > 5 else None
BIG = bytes((i * 7 + i // 256) % 251 for i in range(1 << 20))
PAGE = b"""<!doctype html><html><head><title>Huldra test page</title></head>
<body><h1>Test page</h1><p>Hello from the <b>test</b> server. <a href="/hello.txt">a text file</a>
and <a href="/second.html">the second page</a>.</p>
<ul><li>first item</li><li>second item</li></ul>
<form action="/echo" method="post"><input name="q" value="huldra"><input type="submit" value="Send"></form>
</body></html>"""
SECOND = b"<html><head><title>Second</title></head><body><h2>Second page</h2><p><a href='page.html'>back</a></p></body></html>"


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def send(self, code, body, ctype="text/plain", extra=()):
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        for k, v in extra:
            self.send_header(k, v)
        self.send_header("Connection", "close")
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def git(self, body=b""):
        path, _, query = self.path[4:].partition("?")
        env = dict(os.environ, GIT_PROJECT_ROOT=GIT_ROOT, GIT_HTTP_EXPORT_ALL="1", PATH_INFO=path, QUERY_STRING=query,
                   REQUEST_METHOD=self.command, CONTENT_TYPE=self.headers.get("Content-Type", ""),
                   CONTENT_LENGTH=str(len(body)), REMOTE_ADDR="10.0.2.15", REMOTE_USER="huldra")
        out = subprocess.run(["git", "http-backend"], input=body, env=env, capture_output=True).stdout
        head, sep, data = out.partition(b"\r\n\r\n")
        if not sep:
            head, sep, data = out.partition(b"\n\n")
        status, headers = 200, []
        for line in head.decode("latin-1").splitlines():
            k, _, v = line.partition(":")
            if k.lower() == "status":
                status = int(v.split()[0])
            elif k:
                headers.append((k, v.strip()))
        self.send(status, data, dict(headers).get("Content-Type", "application/octet-stream"), [h for h in headers if h[0] != "Content-Type"])

    def do_GET(self):
        https = isinstance(self.connection, ssl.SSLSocket)
        p = self.path
        if p.startswith("/git/") and GIT_ROOT:
            self.git()
            return
        if p == "/hello.txt":
            self.send(200, b"hello over %s\n" % (b"https" if https else b"http"))
        elif p == "/redirect":
            target = "/hello.txt" if https else "https://10.0.2.2:%d/hello.txt" % HTTPS_PORT
            self.send(302, b"", extra=[("Location", target)])
        elif p == "/chunked":
            self.send_response(200)
            self.send_header("Transfer-Encoding", "chunked")
            self.send_header("Connection", "close")
            self.end_headers()
            for part in (b"chunked ", b"transfer ", b"works\n"):
                self.wfile.write(b"%x\r\n%s\r\n" % (len(part), part))
            self.wfile.write(b"0\r\n\r\n")
        elif p == "/gzip":
            if "gzip" in self.headers.get("Accept-Encoding", ""):
                self.send(200, gzip.compress(b"gzip decoded fine\n" * 50), extra=[("Content-Encoding", "gzip")])
            else:
                self.send(200, b"gzip decoded fine\n" * 50)
        elif p == "/big":
            rng = self.headers.get("Range", "")
            if rng.startswith("bytes="):
                start = int(rng[6:].split("-")[0])
                if start >= len(BIG):
                    self.send(416, b"")
                    return
                self.send(206, BIG[start:], "application/octet-stream", [("Content-Range", "bytes %d-%d/%d" % (start, len(BIG) - 1, len(BIG)))])
            else:
                self.send(200, BIG, "application/octet-stream")
        elif p in ("/", "/page.html"):
            self.send(200, PAGE, "text/html; charset=utf-8")
        elif p == "/second.html":
            self.send(200, SECOND, "text/html")
        else:
            self.send(404, b"not found\n")

    def do_HEAD(self):
        self.do_GET()

    def do_POST(self):
        n = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(n)
        if self.path.startswith("/git/") and GIT_ROOT:
            self.git(body)
        elif self.path == "/echo":
            self.send(200, b"you sent: " + body + b"\n")
        else:
            self.send(404, b"not found\n")


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = True


plain = Server(("127.0.0.1", HTTP_PORT), Handler)
secure = Server(("127.0.0.1", HTTPS_PORT), Handler)
ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
ctx.minimum_version = ssl.TLSVersion.TLSv1_3
ctx.load_cert_chain(CERT, KEY)
secure.socket = ctx.wrap_socket(secure.socket, server_side=True)
threading.Thread(target=plain.serve_forever, daemon=True).start()
print("ready", flush=True)
secure.serve_forever()
