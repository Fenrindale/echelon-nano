"""Owned, synthetic, internal-only API for broker and work scenario verification."""
from http.server import BaseHTTPRequestHandler, HTTPServer
import base64
import json


class Handler(BaseHTTPRequestHandler):
    counts = {}

    def log_message(self, *args):
        pass  # Request headers and credentials must never reach container logs.

    def do_GET(self):
        self.counts[self.path] = self.counts.get(self.path, 0) + 1
        auth = self.headers.get("Authorization", "")
        if self.path == "/snapshot":
            data = {"fixture": True, "week": "synthetic-week-41", "requests": 120,
                    "resolved": 108, "pending": 12, "success_rate": 0.9,
                    "authenticated": bool(auth), "source": "owned synthetic API"}
        elif self.path == "/echo":
            data = {"fixture": True, "echo": auth,
                    "encoded": base64.b64encode(auth.encode()).decode(),
                    "access_token": "owned-echo-only", "request_count": self.counts[self.path]}
        elif self.path == "/whoAmI/api/json":
            expected = "Basic " + base64.b64encode(b"owned-user:owned-identity-token").decode()
            valid = auth == expected
            data = {"authenticated": valid, "name": "owned-user" if valid else "anonymous",
                    "request_count": self.counts[self.path]}
        elif self.path == "/redirect":
            self.send_response(302)
            self.send_header("Location", "http://vault:8200/v1/sys/health")
            self.end_headers()
            return
        else:
            self.send_response(404)
            self.end_headers()
            return
        body = json.dumps(data).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


HTTPServer(("0.0.0.0", 8081), Handler).serve_forever()
