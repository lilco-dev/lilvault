"""Run with: python3 -m unittest discover -s tests -p 'test_cli.py' -v."""

import json
import os
import subprocess
import sys
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

CLI = Path(__file__).resolve().parents[1] / "scripts/lilvault"
TOKEN = "b" * 64  # Deliberately fake; no real credentials in fixtures.
STATE = {}


class Handler(BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        pass

    def do_POST(self):
        payload = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        STATE["requests"].append((payload, dict(self.headers)))
        if STATE.get("http_error"):
            self.send_response(401)
            self.end_headers()
            return
        if STATE.get("redirect"):
            self.send_response(302)
            self.send_header("Location", "/redirect-target")
            self.end_headers()
            return
        method = payload["method"]
        if method == "notifications/initialized":
            self.send_response(202)
            self.end_headers()
            return
        result = {"protocolVersion": STATE.get("protocol", "2025-06-18"), "capabilities": {}, "serverInfo": {"name": "fixture", "version": "1"}}
        if method == "tools/list":
            result = {"tools": [{"name": "read_file", "description": "Read a note"}]}
        elif method == "tools/call":
            result = {"content": [{"type": "text", "text": '{"ok": true}'}], "isError": STATE.get("tool_error", False)}
        response = {"jsonrpc": "2.0", "id": payload["id"], "result": result}
        if STATE.get("rpc_error") and method != "initialize":
            response = {"jsonrpc": "2.0", "id": payload["id"], "error": {"code": -32602, "message": "Invalid arguments"}}
        if STATE.get("mismatched_id"):
            response["id"] = 999
        if STATE.get("invalid_response"):
            body = b"not a protocol response"
        elif STATE.get("sse"):
            body = ("event: message\r\ndata: " + json.dumps(response, indent=2).replace("\n", "\r\ndata: ") + "\r\n\r\n").encode()
        else:
            body = json.dumps(response).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream" if STATE.get("sse") else "application/json")
        if STATE.get("session") and method == "initialize":
            self.send_header("Mcp-Session-Id", "fixture-session")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        STATE["redirect_followed"] = True
        self.send_response(200)
        self.end_headers()


class CliTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join()

    def setUp(self):
        STATE.clear()
        STATE["requests"] = []
        self.env = {key: value for key, value in os.environ.items() if not key.startswith("LILVAULT_")}
        self.env.update(LILVAULT_TOKEN=TOKEN, LILVAULT_URL=f"http://127.0.0.1:{self.server.server_port}/mcp")

    def run_cli(self, *args, stdin=None):
        return subprocess.run([sys.executable, str(CLI), *args], input=stdin, text=True, capture_output=True, env=self.env, timeout=10)

    def test_help_needs_no_token_or_connection(self):
        self.env.pop("LILVAULT_TOKEN")
        result = self.run_cli("--help")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("tools", result.stdout)
        self.assertFalse(STATE["requests"])

    def test_tools_performs_authenticated_mcp_handshake(self):
        # An invalid ambient proxy must not hijack local/Tailnet requests.
        self.env.update(http_proxy="http://127.0.0.1:1", HTTP_PROXY="http://127.0.0.1:1", no_proxy="", NO_PROXY="")
        STATE["session"] = True
        result = self.run_cli("tools")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("read_file", result.stdout)
        requests = STATE["requests"]
        self.assertEqual([p["method"] for p, _ in requests], ["initialize", "notifications/initialized", "tools/list"])
        self.assertNotIn("id", requests[1][0])
        self.assertNotIn("Mcp-Session-Id", requests[0][1])
        for _, headers in requests:
            self.assertEqual(headers["Authorization"], "Bearer " + TOKEN)
        for _, headers in requests[1:]:
            self.assertEqual(headers["Mcp-Session-Id"], "fixture-session")
            self.assertEqual(headers["Mcp-Protocol-Version"], "2025-06-18")

    def test_token_file_and_write_arguments_from_stdin(self):
        self.env.pop("LILVAULT_TOKEN")
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "token"
            path.write_text(TOKEN + "\n")
            path.chmod(0o600)
            self.env["LILVAULT_TOKEN_FILE"] = str(path)
            arguments = {"vault_id": "test", "path": "hello.md", "content": "Hello", "create_only": True}
            result = self.run_cli("call", "write_file", "-", stdin=json.dumps(arguments))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), {"ok": True})
        self.assertEqual(STATE["requests"][-1][0]["params"], {"name": "write_file", "arguments": arguments})
        self.assertNotIn(TOKEN, result.stdout + result.stderr)

    def test_revision_is_forwarded_unchanged(self):
        arguments = {"vault_id": "test", "path": "hello.md", "content": "Update", "expected_revision": "a" * 64}
        result = self.run_cli("call", "write_file", json.dumps(arguments))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(STATE["requests"][-1][0]["params"]["arguments"], arguments)

    def test_buffered_multiline_sse_responses(self):
        STATE["sse"] = True
        result = self.run_cli("call", "list_vaults")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), {"ok": True})

    def test_rpc_tool_and_http_errors_exit_nonzero(self):
        for flag in ["rpc_error", "tool_error", "http_error", "invalid_response", "mismatched_id"]:
            with self.subTest(flag=flag):
                STATE.clear()
                STATE.update(requests=[], **{flag: True})
                result = self.run_cli("call", "read_file", '{}')
                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn("Traceback", result.stderr)
                self.assertNotIn(TOKEN, result.stdout + result.stderr)

    def test_redirects_do_not_receive_credentials(self):
        STATE["redirect"] = True
        result = self.run_cli("tools")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(len(STATE["requests"]), 1)
        self.assertNotIn("redirect_followed", STATE)

    def test_bad_configuration_fails_before_network(self):
        for url in ["http://example.invalid/mcp", "ftp://127.0.0.1/mcp", "https://user:password@example.invalid/mcp", "https://example.invalid/mcp?token=x"]:
            with self.subTest(url=url):
                self.env["LILVAULT_URL"] = url
                result = self.run_cli("tools")
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(STATE["requests"])
        self.env["LILVAULT_URL"] = f"http://127.0.0.1:{self.server.server_port}/mcp"
        self.env.pop("LILVAULT_TOKEN")
        self.assertNotEqual(self.run_cli("tools").returncode, 0)
        self.assertFalse(STATE["requests"])

    def test_bad_arguments_fail_before_network(self):
        for arguments in ["not json", "[]", "null"]:
            with self.subTest(arguments=arguments):
                result = self.run_cli("call", "write_file", arguments)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(STATE["requests"])

    def test_unsupported_negotiated_protocol_fails_closed(self):
        STATE["protocol"] = "1900-01-01"
        result = self.run_cli("tools")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(len(STATE["requests"]), 1)


if __name__ == "__main__":
    unittest.main()
