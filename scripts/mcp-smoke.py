"""Speak just enough MCP to prove the server starts without a window.

Spawns the binary, sends `initialize`, then `tools/list`, and prints what came
back. Exits non-zero if either answer is missing, so it can gate a package.
"""
import json
import os
import subprocess
import sys
import threading


def main() -> int:
    binary = sys.argv[1] if len(sys.argv) > 1 else "shale-mcp.exe"
    child = subprocess.Popen(
        [binary],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        bufsize=1,
        env={**os.environ},
    )
    requests = [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "shale-smoke", "version": "0"},
        }},
        {"jsonrpc": "2.0", "method": "notifications/initialized"},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}},
    ]
    for request in requests:
        child.stdin.write(json.dumps(request) + "\n")
    child.stdin.flush()

    seen = {}
    def reader():
        for line in child.stdout:
            line = line.strip()
            if not line:
                continue
            try:
                message = json.loads(line)
            except json.JSONDecodeError:
                continue
            if message.get("id") in (1, 2):
                seen[message["id"]] = message
                if len(seen) == 2:
                    return
    thread = threading.Thread(target=reader, daemon=True)
    thread.start()
    thread.join(timeout=30)
    child.kill()

    if 1 not in seen:
        print("initialize: no answer", file=sys.stderr)
        print(child.stderr.read()[-2000:], file=sys.stderr)
        return 1
    server = seen[1].get("result", {}).get("serverInfo", {})
    print(f"initialize ok: {server.get('name')} {server.get('version')}")
    if 2 not in seen:
        print("tools/list: no answer", file=sys.stderr)
        return 1
    tools = seen[2].get("result", {}).get("tools", [])
    print(f"tools: {len(tools)}")
    for tool in tools:
        print("  " + tool.get("name", "?"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
