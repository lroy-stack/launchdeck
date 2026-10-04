"""A stand-in for Home Assistant: the REST endpoints the example plugin uses.

    GET  /api/states/<entity_id>             the entity, or 404
    POST /api/services/<domain>/<service>    turn_on, turn_off, toggle

Every request needs `Authorization: Bearer <token>`; anything else is 401. It
listens on a loopback port and remembers every request, so a step can prove
what reached it and what did not.
"""

import json
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class MockHomeAssistant:
    def __init__(self, token, entities, port=0):
        """`port` asks for one port in particular; if it is taken, any free one is used."""
        self.token = token
        self.entities = dict(entities)
        self.requests = []  # (method, path, entity or None, authorized)
        self.delay = 0.0
        self.lock = threading.Lock()
        mock = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def answer(self, status, body):
                data = json.dumps(body).encode()
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

            def entity(self, entity_id, state):
                return {"entity_id": entity_id, "state": state, "attributes": {"friendly_name": entity_id}, "last_changed": "2026-10-04T19:00:00+00:00", "last_updated": "2026-10-04T19:00:00+00:00"}

            def handle_any(self, method):
                length = int(self.headers.get("Content-Length") or 0)
                raw = self.rfile.read(length) if length else b""
                try:
                    body = json.loads(raw) if raw else {}
                except ValueError:
                    body = {}
                authorized = self.headers.get("Authorization") == f"Bearer {mock.token}"
                with mock.lock:
                    mock.requests.append((method, self.path, body.get("entity_id"), authorized))
                if mock.delay:
                    time.sleep(mock.delay)
                if not authorized:
                    return self.answer(401, {"message": "401: Unauthorized"})
                path = self.path.split("?")[0]
                if method == "GET" and path.startswith("/api/states/"):
                    entity_id = path[len("/api/states/"):]
                    with mock.lock:
                        state = mock.entities.get(entity_id)
                    if state is None:
                        return self.answer(404, {"message": "Entity not found."})
                    return self.answer(200, self.entity(entity_id, state))
                if method == "POST" and path.startswith("/api/services/"):
                    parts = path[len("/api/services/"):].split("/")
                    entity_id = body.get("entity_id")
                    if len(parts) != 2 or parts[1] not in ("turn_on", "turn_off", "toggle") or not entity_id or not entity_id.startswith(parts[0] + "."):
                        return self.answer(400, {"message": "Service not found."})
                    with mock.lock:
                        now = mock.entities.get(entity_id)
                        if now is None:
                            return self.answer(200, [])
                        mock.entities[entity_id] = {"turn_on": "on", "turn_off": "off"}.get(parts[1], "off" if now == "on" else "on")
                        return self.answer(200, [self.entity(entity_id, mock.entities[entity_id])])
                return self.answer(404, {"message": "404: Not Found"})

            def do_GET(self):
                self.handle_any("GET")

            def do_POST(self):
                self.handle_any("POST")

        try:
            self.server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
        except OSError:
            self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.port = self.server.server_address[1]
        self.origin = f"http://127.0.0.1:{self.port}"
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def stop(self):
        self.server.shutdown()
        self.server.server_close()

    def set(self, entity_id, state):
        """Change an entity from outside, as a wall switch would."""
        with self.lock:
            self.entities[entity_id] = state

    def state(self, entity_id):
        with self.lock:
            return self.entities.get(entity_id)

    def hits(self):
        with self.lock:
            return len(self.requests)

    def service_calls(self):
        with self.lock:
            return [f"{path[len('/api/services/'):]} {entity}" for method, path, entity, _ in self.requests if method == "POST"]
