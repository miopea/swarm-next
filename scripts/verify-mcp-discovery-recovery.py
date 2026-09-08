"""Fictional loopback verification; never connects to a Hive or reads credentials."""

import json
import os
import subprocess
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


def verify(binary, method, statuses, expected_calls, expect_error):
    received = []

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_POST(self):
            size = int(self.headers.get("Content-Length", "0"))
            if size > 8192:
                self.send_error(413)
                return
            request = json.loads(self.rfile.read(size))
            received.append(request)
            status = statuses[min(len(received) - 1, len(statuses) - 1)]
            body = json.dumps({"jsonrpc": "2.0", "id": request["id"],
                               "result": {"fixture": "recovered"}}).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    server.daemon_threads = True
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    request = {"jsonrpc": "2.0", "id": "fictional-request-17", "method": method,
               "params": {"fixture": "no-real-task-or-customer"}}
    try:
        env = dict(os.environ, SWARM_MCP_URL=f"http://127.0.0.1:{server.server_port}/mcp",
                   SWARM_MCP_AUTHORIZATION="Bearer fictional-loopback-only")
        result = subprocess.run([binary, "mcp-proxy"], input=json.dumps(request) + "\n",
                                text=True, capture_output=True, env=env, timeout=25, check=True)
        responses = [json.loads(line) for line in result.stdout.splitlines() if line.strip()]
        assert len(responses) == 1, "one response per provider request"
        response = responses[0]
        assert response["id"] == request["id"], "preserve provider request identity"
        assert ("error" in response) == expect_error, "never disguise exhaustion as empty tools"
        assert len(received) == expected_calls, f"{method}: unexpected replay count {len(received)}"
        assert all(item == request for item in received), "retry only the original discovery request"
        print(f"PASS {method}: statuses={statuses}, attempts={len(received)}, error={expect_error}")
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("Usage: verify-mcp-discovery-recovery.py /absolute/path/to/swarm-terminal-host")
    binary = sys.argv[1]
    if not os.path.isabs(binary):
        raise SystemExit("Supply an absolute isolated build path, not a live service command")
    verify(binary, "initialize", [503, 200], 2, False)
    verify(binary, "tools/list", [502, 200], 2, False)
    verify(binary, "tools/call", [503, 200], 1, True)
    verify(binary, "initialize", [401, 200], 1, True)
    verify(binary, "tools/list", [503], 6, True)
