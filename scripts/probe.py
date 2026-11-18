"""Tiny CDP probe: click things and take screenshots of the running app.

Usage: python probe.py shot out.png | click TEXT | js EXPR
"""
import base64, json, sys, time, urllib.request
from websockets.sync.client import connect


def page():
    for _ in range(120):
        try:
            targets = json.load(urllib.request.urlopen("http://127.0.0.1:9222/json", timeout=2))
            for target in targets:
                if target.get("type") == "page" and "tauri.localhost" in target.get("url", ""):
                    return target["webSocketDebuggerUrl"]
        except Exception:
            pass
        time.sleep(0.5)
    raise SystemExit("no page on 9222")


class Cdp:
    def __init__(self):
        self.ws = connect(page(), max_size=None)
        self.n = 0

    def call(self, method, **params):
        self.n += 1
        self.ws.send(json.dumps({"id": self.n, "method": method, "params": params}))
        while True:
            message = json.loads(self.ws.recv())
            if message.get("id") == self.n:
                if "error" in message:
                    raise SystemExit(message["error"])
                return message.get("result", {})

    def js(self, expression):
        result = self.call("Runtime.evaluate", expression=expression, awaitPromise=True, returnByValue=True)
        return result.get("result", {}).get("value")

    def shot(self, path):
        data = self.call("Page.captureScreenshot", format="png")["data"]
        with open(path, "wb") as handle:
            handle.write(base64.b64decode(data))
        return path


if __name__ == "__main__":
    cdp = Cdp()
    command = sys.argv[1]
    if command == "shot":
        print(cdp.shot(sys.argv[2]))
    elif command == "click":
        text = sys.argv[2]
        print(cdp.js("(() => { const b=[...document.querySelectorAll('button,summary,a,[role=tab]')].find(e=>e.textContent.trim().toLowerCase()===" + json.dumps(text.lower()) + "); if(!b) return 'no'; b.click(); return 'clicked'; })()"))
    elif command == "key":
        key = sys.argv[2]
        codes = {"Escape": (27, "Escape"), "f": (70, "KeyF")}
        vk, code = codes.get(key, (0, key))
        cdp.call("Input.dispatchKeyEvent", type="keyDown", key=key, code=code, windowsVirtualKeyCode=vk)
        cdp.call("Input.dispatchKeyEvent", type="keyUp", key=key, code=code, windowsVirtualKeyCode=vk)
        print("sent", key)
    else:
        print(cdp.js(sys.argv[2]))
