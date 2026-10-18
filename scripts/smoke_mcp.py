#!/usr/bin/env python3
"""Exercise a real stdio MCP server, including persistence after restart."""
import json
import pathlib
import selectors
import subprocess
import sys
import tempfile
import time


class Client:
    def __init__(self, binary, document=None, extra=(), name="cardboard-smoke"):
        self.process = subprocess.Popen(
            [str(binary), "mcp"] + (["--document", str(document)] if document is not None else [])
            + list(extra),
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True,
        )
        self.next_id = 0
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.process.stdout, selectors.EVENT_READ)
        self.request("initialize", {
            "protocolVersion": "2024-11-05", "capabilities": {},
            "clientInfo": {"name": name, "version": "1"},
        })
        self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def send(self, message):
        self.process.stdin.write(json.dumps(message) + "\n")
        self.process.stdin.flush()

    def request(self, method, params):
        self.next_id += 1
        self.send({"jsonrpc": "2.0", "id": self.next_id,
                   "method": method, "params": params})
        while True:
            if not self.selector.select(timeout=10):
                raise RuntimeError(f"MCP response timed out: {method}")
            line = self.process.stdout.readline()
            if not line:
                raise RuntimeError(f"MCP server closed stdout: {method}")
            response = json.loads(line)
            if response.get("id") == self.next_id:
                if "error" in response:
                    raise RuntimeError(response["error"])
                return response["result"]

    def tool(self, name, arguments):
        return self.request("tools/call", {"name": name, "arguments": arguments})

    def value(self, name, arguments):
        result = self.tool(name, arguments)
        assert not result.get("isError"), result
        return result["structuredContent"]

    def close(self):
        self.process.stdin.close()
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.terminate()
            self.process.wait(timeout=5)
        finally:
            self.selector.close()
            self.process.stdout.close()


def main():
    root = pathlib.Path(__file__).resolve().parents[1]
    binary = pathlib.Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else root / "target/debug/cardboard"
    with tempfile.TemporaryDirectory(prefix="cardboard-mcp-") as temporary:
        document = pathlib.Path(temporary) / "design.cardboard"
        client = Client(binary, document)
        try:
            tools = client.request("tools/list", {})["tools"]
            assert len(tools) == 54, len(tools)
            guide = client.value("get_guide", {"topic": "cardboard-design"})
            assert guide["guide"]
            file_id = client.value("list_resources", {})["resources"][0]["id"]
            frame = client.value("create_artboard", {
                "fileId": file_id, "name": "Desktop",
                "styles": {"width": "1440px", "height": "900px"},
            })["nodeId"]
            child = client.value("write_html", {
                "fileId": file_id, "targetNodeId": frame, "mode": "insert-children",
                "html": '<div style="padding:32px"><h1>Hello &amp; Cardboard</h1></div>',
            })["nodeIds"][0]
            client.value("update_styles", {
                "fileId": file_id,
                "updates": [{"nodeIds": [child], "styles": {"backgroundColor": "#f4f4f4"}}],
            })
            hits = client.value("find_nodes", {
                "fileId": file_id, "styles": {"background-color": "#f4f4f4"},
            })["nodes"]
            assert [h["id"] for h in hits] == [child], hits
            pixel = ("data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAA"
                     "DUlEQVR4nGNgYPj/HwADAgH/eL9GtQAAAABJRU5ErkJggg==")
            image = client.value("import_image", {
                "fileId": file_id, "dataUri": pixel, "parentId": frame, "alt": "Pixel",
            })
            assert image["src"].startswith("/asset/") and image["width"] == 1, image
            exported = client.value("export_html", {"fileId": file_id})["html"]
            assert pixel.split(",")[1] in exported and "/asset/" not in exported
            assert "Hello &amp; Cardboard" in exported and "background-color:#f4f4f4" in exported
            assert "data-cardboard-id" not in exported
            failure = client.tool("move_nodes", {
                "fileId": file_id, "moves": [{"nodeId": frame, "parentId": child}],
            })
            assert failure.get("isError"), failure
            # A failed command must leave the connection and model usable.
            assert client.value("export_html", {"fileId": file_id})["html"] == exported
            shot = client.tool("screenshot", {"fileId": file_id, "nodeId": frame})
            if shot.get("isError"):
                assert "none was found" in shot["content"][0]["text"], shot
                print("screenshot skipped: no Chromium-based browser")
            else:
                image = [c for c in shot["content"] if c["type"] == "image"]
                assert image and image[0]["mimeType"] == "image/png", shot["content"][0]
                assert "image" not in shot["structuredContent"]
        finally:
            client.close()
        reopened = Client(binary, document)
        try:
            assert reopened.value("get_node_info", {"fileId": file_id, "nodeId": child})["id"] == child
            assert reopened.value("export_html", {"fileId": file_id})["html"] == exported
        finally:
            reopened.close()
    relay(binary)
    print("MCP smoke passed: 54 tools, editing, search, image assets, recoverable errors, export, restart persistence")


def relay(binary):
    """No app running: the relay hosts headless, the app takes over and leaves, the agent carries on."""
    with tempfile.TemporaryDirectory(prefix="cardboard-relay-") as temporary:
        root = pathlib.Path(temporary)
        flags = ["--socket", str(root / "cardboard.sock"), "--library", str(root / "library")]
        a = Client(binary, extra=flags, name="agent-a")
        b = None
        serve = None
        try:
            file_id = a.value("create_file", {"name": "Relay"})["fileId"]
            info = a.value("get_basic_info", {"fileId": file_id})
            assert info["connection"]["mode"] == "headless", info
            assert info["connection"]["editorUrl"].startswith("http://127.0.0.1:"), info
            frame = a.value("create_artboard", {
                "fileId": file_id, "name": "A", "styles": {"width": "100px", "height": "100px"},
            })["nodeId"]

            # A second agent shares the headless host; A's artboard is claimed.
            b = Client(binary, extra=flags, name="agent-b")
            blocked = b.tool("update_styles", {
                "fileId": file_id, "updates": [{"nodeIds": [frame], "styles": {"color": "red"}}],
            })
            assert blocked.get("isError") and "agent-a" in blocked["content"][0]["text"], blocked
            other = b.value("create_artboard", {
                "fileId": file_id, "name": "B", "styles": {"width": "100px", "height": "100px"},
            })["nodeId"]
            # A cannot undo B's edit.
            assert a.tool("undo", {"fileId": file_id}).get("isError")
            b.value("finish_working_on_nodes", {"fileId": file_id})

            # The app (here `serve`) starts: the headless host saves and hands over.
            serve = subprocess.Popen([str(binary), "serve", "--port", "0"] + flags,
                                     stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
            serve.stdout.readline()
            for _ in range(50):
                info = a.tool("get_basic_info", {"fileId": file_id})
                if not info.get("isError") and info["structuredContent"]["connection"]["mode"] == "serve":
                    break
                time.sleep(0.1)
            assert info["structuredContent"]["connection"]["mode"] == "serve", info
            assert a.value("get_node_info", {"fileId": file_id, "nodeId": other})["name"] == "B"
            a.value("rename_nodes", {"fileId": file_id, "updates": [{"nodeId": frame, "name": "A2"}]})

            # The app quits: an agent takes the library back and nothing is lost.
            serve.terminate()
            serve.wait(timeout=5)
            for _ in range(50):
                info = a.tool("get_basic_info", {"fileId": file_id})
                if not info.get("isError") and info["structuredContent"]["connection"]["mode"] == "headless":
                    break
                time.sleep(0.1)
            assert info["structuredContent"]["connection"]["mode"] == "headless", info
            assert a.value("get_node_info", {"fileId": file_id, "nodeId": frame})["name"] == "A2"
        finally:
            for client in (a, b):
                if client:
                    client.close()
            if serve and serve.poll() is None:
                serve.terminate()
                serve.wait(timeout=5)
        assert not (root / "cardboard.sock").exists(), "the last host should remove its socket"


if __name__ == "__main__":
    main()
