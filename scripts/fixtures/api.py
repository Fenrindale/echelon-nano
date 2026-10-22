"""Owned, synthetic, internal-only API for broker and work scenario verification."""
from http.server import BaseHTTPRequestHandler, HTTPServer
import base64
import json


class Handler(BaseHTTPRequestHandler):
    counts = {}
    resolved = set()

    def do_POST(self):
        if self.path not in ('/mcp', '/mcp-lost'):
            self.send_error(404)
            return
        message = json.loads(self.rfile.read(int(self.headers.get('Content-Length', 0))))
        method = message.get('method')
        if 'id' not in message:
            self.send_response(202)
            self.end_headers()
            return
        if method == 'initialize':
            result = {'protocolVersion': '2025-03-26', 'capabilities': {'tools': {}},
                      'serverInfo': {'name': 'owned-fixture', 'version': '1'}}
        elif method == 'tools/list':
            result = {'tools': [
                {'name': 'list_items', 'description': 'List owned synthetic unresolved work items', 'inputSchema': {'type': 'object', 'properties': {}}},
                {'name': 'resolve_item', 'description': 'Resolve one owned synthetic work item', 'inputSchema': {'type': 'object', 'properties': {'id': {'type': 'string'}}, 'required': ['id']}}
            ]}
        elif method == 'tools/call':
            name = message['params']['name']
            self.counts[name] = self.counts.get(name, 0) + 1
            if name == 'resolve_item':
                self.resolved.add(message['params']['arguments']['id'])
                if self.path == '/mcp-lost':
                    self.close_connection = True
                    return
            result = {'content': [{'type': 'text', 'text': json.dumps({'fixture': True, 'items': ['owned-item-A', 'owned-item-B'], 'resolved': sorted(self.resolved), 'call_count': self.counts[name], 'auth_echo': self.headers.get('Authorization', '')})}]}
        else:
            self.send_error(400)
            return
        body = json.dumps({'jsonrpc': '2.0', 'id': message['id'], 'result': result}).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        payload = b'data: ' + body + b'\n\n'
        self.send_header('Content-Length', str(len(payload)))
        self.send_header('Mcp-Session-Id', 'owned-fixture-session')
        self.end_headers()
        self.wfile.write(payload)

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
