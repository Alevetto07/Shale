"""A fake Bambu cloud on loopback, for the probe.

It answers the five calls the client makes, with the shapes the client expects,
and nothing else. It exists so the sign-in, the code step, the device list and
the send can be driven in a real window without an account and without touching
Bambu: the same reason `shale-fake-printer` exists.

    python scripts/fake-cloud.py --port 8791
    SHALE_CLOUD_HOST=http://127.0.0.1:8791 SHALE_CLOUD_STORE=memory Shale.exe

Accounts: any address, password `code` to be asked for a code, anything else to
be signed in at once. The code is 123456. Tasks get ids from 700 up.
"""

import argparse
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

TASKS = {"next": 700}


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def _answer(self, status, payload):
        body = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _body(self):
        length = int(self.headers.get("Content-Length") or 0)
        return self.rfile.read(length) if length else b""

    def do_GET(self):
        if self.path == "/v1/iot-service/api/user/device/list":
            return self._answer(
                200,
                {
                    "devices": [
                        {
                            "dev_id": "FAKE01A000000001",
                            "name": "workshop A1",
                            "dev_model_name": "A1",
                            "online": True,
                            "print_status": "IDLE",
                        },
                        {
                            "dev_id": "FAKE01A000000002",
                            "name": "attic P1S",
                            "dev_model_name": "P1S",
                            "online": True,
                            "print_status": "RUNNING",
                            "mc_percent": 37,
                        },
                        {
                            "dev_id": "FAKE01A000000003",
                            "name": "stored A1 mini",
                            "dev_model_name": "A1 mini",
                            "online": False,
                        },
                    ]
                },
            )
        if self.path == "/v1/iot-service/api/user/profile":
            return self._answer(200, {"uid": 1, "name": "probe"})
        return self._answer(404, {"error": f"no route for {self.path}"})

    def do_POST(self):
        raw = self._body()
        if self.path == "/v1/user-service/user/login":
            try:
                payload = json.loads(raw or b"{}")
            except json.JSONDecodeError:
                return self._answer(400, {"error": "not json"})
            if "code" in payload:
                if payload["code"] == "123456":
                    return self._answer(
                        200,
                        {"accessToken": "access-1", "refreshToken": "refresh-1", "expiresIn": 86400},
                    )
                return self._answer(400, {"error": "the code is wrong"})
            if payload.get("password") == "code":
                return self._answer(200, {"loginType": "verifyCode", "tfaKey": "key-1"})
            return self._answer(
                200, {"accessToken": "access-1", "refreshToken": "refresh-1", "expiresIn": 86400}
            )
        if self.path == "/v1/user-service/user/sendemail/code":
            return self._answer(200, {})
        if self.path == "/v1/user-service/user/refreshtoken":
            return self._answer(
                200, {"accessToken": "access-2", "refreshToken": "refresh-2", "expiresIn": 86400}
            )
        if self.path == "/v1/iot-service/api/user/task":
            if b'name="file"' not in raw:
                return self._answer(400, {"error": "the task had no file"})
            task = TASKS["next"]
            TASKS["next"] += 1
            return self._answer(200, {"id": task})
        return self._answer(404, {"error": f"no route for {self.path}"})


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=8791)
    args = parser.parse_args()
    server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    print(f"fake cloud on http://127.0.0.1:{args.port}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
