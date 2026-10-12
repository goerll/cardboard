#!/usr/bin/env python3
"""Run from the host terminal to connect Chrome DevTools MCP to existing Helium."""
import json
import os
from pathlib import Path
import re
import sys
import tempfile
import tomllib
import socket


def main():
    config = Path.home() / ".codex/config.toml"
    active_port = Path.home() / ".config/net.imput.helium/DevToolsActivePort"
    try:
        port = int(active_port.read_text().splitlines()[0])
        if not 1 <= port <= 65535:
            raise ValueError("Invalid debugging port")
        original = config.read_text()
        parsed = tomllib.loads(original)
        current = parsed["mcp_servers"]["chrome-devtools"]
        args = current.get("args", [])
        # Replace only connection flags, retaining unrelated server options.
        updated_args = []
        skip_value = False
        for arg in args:
            if skip_value:
                skip_value = False
                continue
            if arg in ("--browserUrl", "--browser-url", "--wsEndpoint", "--ws-endpoint", "--executablePath", "--executable-path", "--userDataDir", "--user-data-dir"):
                skip_value = True
                continue
            if arg.split("=", 1)[0] in ("--browserUrl", "--browser-url", "--wsEndpoint", "--ws-endpoint", "--executablePath", "--executable-path", "--autoConnect", "--auto-connect", "--userDataDir", "--user-data-dir"):
                continue
            updated_args.append(arg)
        updated_args.extend(["--autoConnect", f"--userDataDir={active_port.parent}"])
        section = re.search(r"(?ms)^\[mcp_servers\.chrome-devtools\]\s*\n.*?(?=^\[|\Z)", original)
        if not section:
            raise ValueError("Expected chrome-devtools MCP section was not found")
        body, count = re.subn(r"(?m)^args[ \t]*=[ \t]*\[[^\n]*\][ \t]*$", "args = " + json.dumps(updated_args), section.group(), count=1)
        if count != 1:
            raise ValueError("Expected a single-line args setting; no config changes made")
        updated = original[:section.start()] + body + original[section.end():]
        assert tomllib.loads(updated)["mcp_servers"]["chrome-devtools"]["args"] == updated_args
        if updated != original:
            backup = config.with_name("config.toml.before-helium")
            if not backup.exists():
                with backup.open("x") as handle:
                    os.chmod(backup, 0o600)
                    handle.write(original)
            with tempfile.NamedTemporaryFile(mode="w", dir=config.parent, delete=False) as handle:
                temporary = Path(handle.name)
                handle.write(updated)
            try:
                temporary.chmod(config.stat().st_mode & 0o777)
                temporary.replace(config)
            finally:
                temporary.unlink(missing_ok=True)
        print("Chrome DevTools MCP configured for approval-based Helium autoConnect.")
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=5):
                pass
            print("Helium's debugging listener is reachable; accept its prompt when MCP connects.")
        except OSError as error:
            print(f"Configuration saved, but the debugging endpoint is not reachable: {error}", file=sys.stderr)
            print("Ensure Helium is running with remote debugging enabled on its reported port.", file=sys.stderr)
            return 1
        print("Restart the Codex session/MCP server to load the new arguments.")
        return 0
    except (OSError, ValueError, KeyError, AssertionError) as error:
        print(f"Could not configure MCP: {error}", file=sys.stderr)
        print("Run this helper in your normal host terminal, outside the agent sandbox.", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
