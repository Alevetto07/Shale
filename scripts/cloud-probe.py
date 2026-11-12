"""Drive the Bambu cloud panel in a running window, against the fake cloud.

Assumes Shale is already up with CDP on 9222, `SHALE_CLOUD_HOST` pointing at
`scripts/fake-cloud.py`, and `SHALE_DEMO_SLICE=1` so there is a plate to send.

    python scripts/cloud-probe.py

Prints one line per step and exits non-zero at the first thing that did not
happen. It signs in twice: once with a password that makes the fake cloud ask
for a code, and once with a plain one.
"""

import json
import sys
import time
import urllib.request

from websockets.sync.client import connect

DEBUG = "http://127.0.0.1:9222"


class Page:
    def __init__(self):
        targets = urllib.request.urlopen(f"{DEBUG}/json/list").read().decode()
        page = next(t for t in json.loads(targets) if t["type"] == "page")
        self.ws = connect(page["webSocketDebuggerUrl"], max_size=None)
        self.n = 0

    def js(self, expression):
        self.n += 1
        self.ws.send(
            json.dumps(
                {
                    "id": self.n,
                    "method": "Runtime.evaluate",
                    "params": {
                        "expression": expression,
                        "returnByValue": True,
                        "awaitPromise": True,
                    },
                }
            )
        )
        while True:
            message = json.loads(self.ws.recv())
            if message.get("id") == self.n:
                result = message.get("result", {}).get("result", {})
                if message.get("result", {}).get("exceptionDetails"):
                    raise RuntimeError(message["result"]["exceptionDetails"])
                return result.get("value")

    def text(self):
        return self.js("document.body.innerText") or ""

    def wait_for(self, needle, seconds=12):
        deadline = time.time() + seconds
        while time.time() < deadline:
            # innerText is what the eye sees, and CSS uppercases some labels, so
            # the comparison is on the folded text.
            if needle.lower() in self.text().lower():
                return True
            time.sleep(0.4)
        return False

    def click(self, label):
        return self.js(
            """(() => {
              const want = %s;
              const nodes = [...document.querySelectorAll('button')];
              const hit = nodes.find((b) => b.innerText.trim().toLowerCase() === want);
              if (!hit) return 'no button: ' + want;
              hit.click();
              return 'clicked ' + want;
            })()"""
            % json.dumps(label.lower())
        )

    def fill(self, index, value):
        return self.js(
            """(() => {
              const inputs = [...document.querySelectorAll('.cloud-form input')];
              const input = inputs[%d];
              if (!input) return 'no input %d';
              const setter = Object.getOwnPropertyDescriptor(
                window.HTMLInputElement.prototype, 'value').set;
              setter.call(input, %s);
              input.dispatchEvent(new Event('input', { bubbles: true }));
              return input.type + '=' + input.value;
            })()"""
            % (index, index, json.dumps(value))
        )


def step(page, label, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + label + (" — " + str(detail) if detail else ""))
    if not ok:
        sys.exit(1)


def main():
    page = Page()
    # The demo preload imports a cube and the import takes the pane to Prepare,
    # so wait for the model before asking for Device: a click that lands first is
    # undone by the import.
    step(page, "window is up", page.wait_for("device", 30), page.text()[:120])
    step(page, "model loaded", page.wait_for("cube.stl", 90), page.text()[:200])
    step(page, "device tab", page.click("device") != "no button: device")
    step(page, "panel is there", page.wait_for("bambu cloud"), page.text()[:200])
    # The panel asks the backend who is signed in before it draws the form.
    step(page, "form is drawn", page.wait_for("sign in", 20), page.text()[:200])

    # Sign in with a password that makes the cloud ask for a code.
    step(page, "email filled", page.fill(0, "probe@example.com").startswith("email"))
    step(page, "password filled", page.fill(1, "code").startswith("password"))
    time.sleep(0.2)
    step(page, "sign in pressed", page.click("sign in") != "no button: sign in")
    step(page, "code asked for", page.wait_for("six-digit code"), page.text()[-300:])

    step(page, "code filled", page.fill(0, "000000").startswith("text"))
    page.click("verify")
    step(page, "wrong code says so", page.wait_for("the code is wrong"), page.text()[-300:])

    step(page, "right code filled", page.fill(0, "123456").startswith("text"))
    page.click("verify")
    step(page, "signed in", page.wait_for("signed in as"), page.text()[-400:])

    step(page, "device list", page.wait_for("workshop A1"), page.text()[-500:])
    text = page.text()
    step(page, "printing machine says how far", "printing, 37%" in text, text[-400:])
    step(page, "offline machine is shown", "stored A1 mini" in text, text[-400:])

    buttons = page.js(
        "[...document.querySelectorAll('.cloud-device')].map((row) => row.innerText.replace(/\\n/g, ' | '))"
    )
    print("rows:", buttons)

    # Nothing has been sliced in this window, so the send must say so rather
    # than upload an empty file.
    page.click("send plate")
    step(page, "send without a slice is refused", page.wait_for("nothing to send"), page.text()[-300:])

    print("OK")
    page.ws.close()


if __name__ == "__main__":
    main()
