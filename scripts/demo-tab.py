"""Click a Shale tab in the running window, for the demo recorder.

The app is started with `--remote-debugging-port=9222`, so its webview can be
driven the same way a person would: find the tab by its text and click it.
Nothing else is touched; the click is the only action.

Usage: python scripts/demo-tab.py PREVIEW
"""

import json
import sys
import time
import urllib.request

from websockets.sync.client import connect

PORT = 9222


def page():
    deadline = time.time() + 20
    while time.time() < deadline:
        try:
            pages = json.load(urllib.request.urlopen(f"http://127.0.0.1:{PORT}/json"))
        except Exception:
            time.sleep(0.3)
            continue
        for entry in pages:
            if entry.get("type") == "page" and "tauri.localhost" in entry.get("url", ""):
                return entry
        time.sleep(0.3)
    return None


def main():
    if len(sys.argv) < 2:
        print("usage: demo-tab.py <TAB>")
        return 2
    wanted = sys.argv[1].upper()
    entry = page()
    if entry is None:
        print("no page")
        return 1
    expression = (
        "(() => {"
        f" const wanted = {json.dumps(wanted)};"
        " const tabs = [...document.querySelectorAll('button')];"
        " const tab = tabs.find((b) => b.textContent.trim().toUpperCase() === wanted);"
        " if (!tab) return 'missing';"
        " if (tab.getAttribute('aria-selected') === 'true') return 'active';"
        " tab.click();"
        " return 'clicked';"
        "})()"
    )
    with connect(entry["webSocketDebuggerUrl"], open_timeout=20) as ws:
        ws.send(json.dumps({"id": 1, "method": "Runtime.evaluate", "params": {"expression": expression, "returnByValue": True}}))
        answer = json.loads(ws.recv())
    print(answer.get("result", {}).get("result", {}).get("value", "no answer"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
