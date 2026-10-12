#!/usr/bin/env python3
"""Create a starter .cardboard file through MCP; never overwrite an existing file."""
import pathlib
import sys
from smoke_mcp import Client

root = pathlib.Path(__file__).resolve().parents[1]
if len(sys.argv) != 2:
    sys.exit("Usage: python3 scripts/create-demo.py /absolute/path/to/workspace.json")
document = pathlib.Path(sys.argv[1]).expanduser().resolve()
if document.exists():
    sys.exit(f"File already exists; not overwriting: {document}")
document.parent.mkdir(parents=True, exist_ok=True)
client = Client(root / "target/debug/cardboard", document)
try:
    file_id = client.value("list_resources", {})["resources"][0]["id"]
    frame = client.value("create_artboard", {
        "fileId": file_id, "name": "First canvas",
        "styles": {"width": "1280px", "height": "832px", "backgroundColor": "#f5f3ee"},
    })["nodeId"]
    client.value("write_html", {
        "fileId": file_id, "targetNodeId": frame, "mode": "insert-children",
        "html": (root / "examples/starter.html").read_text(),
    })
finally:
    client.close()
print(f"Starter file saved to {document}")
