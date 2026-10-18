#!/usr/bin/env python3
"""Exercise a real stdio MCP server, including persistence after restart."""
import json
import pathlib
import selectors
import subprocess
import sys
import tempfile


class Client:
    def __init__(self, binary, document=None):
        self.process = subprocess.Popen(
            [str(binary), "mcp"] + (["--document", str(document)] if document is not None else []),
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True,
        )
        self.next_id = 0
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.process.stdout, selectors.EVENT_READ)
        self.request("initialize", {
            "protocolVersion": "2024-11-05", "capabilities": {},
            "clientInfo": {"name": "cardboard-smoke", "version": "1"},
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
            assert len(tools) == 47, len(tools)
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
            exported = client.value("export_html", {"fileId": file_id})["html"]
            assert "Hello &amp; Cardboard" in exported and "background-color:#f4f4f4" in exported
            assert "data-cardboard-id" not in exported
            failure = client.tool("move_nodes", {
                "fileId": file_id, "moves": [{"nodeId": frame, "parentId": child}],
            })
            assert failure.get("isError"), failure
            # A failed command must leave the connection and model usable.
            assert client.value("export_html", {"fileId": file_id})["html"] == exported
        finally:
            client.close()
        reopened = Client(binary, document)
        try:
            assert reopened.value("get_node_info", {"fileId": file_id, "nodeId": child})["id"] == child
            assert reopened.value("export_html", {"fileId": file_id})["html"] == exported
        finally:
            reopened.close()
    print("MCP smoke passed: 47 tools, editing, search, recoverable errors, export, restart persistence")


if __name__ == "__main__":
    main()
