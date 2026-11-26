import { useEffect, useState } from "react";
import { toast } from "sonner";
import * as api from "./api";
import { PrintOptionFields } from "./send-review";
import { Button, Input } from "./ui";

/// The Bambu account, in the Device pane.
///
/// Signing in is three steps and one of them only happens when Bambu asks for
/// it: the address and the password, then the six-digit code that arrives by
/// mail. The panel never keeps the password — it goes to the backend once and
/// is forgotten here — and the tokens live in the Windows Credential Manager,
/// so this file has nothing to store.
///
/// The device list is the cloud's own list: these are the machines registered
/// to the account, whether or not they are on this network. A machine that is
/// offline is shown and cannot be sent to, because a task queued for a machine
/// nobody can see is a mystery, not a print.
export function CloudPanel({
  taskName,
  options,
  onOptions,
  mapping,
  material,
  colour,
  nozzle,
}: {
  taskName: string;
  options?: api.PrintOptions;
  onOptions?: (next: api.PrintOptions) => void;
  mapping?: number[];
  material?: string;
  colour?: string;
  nozzle?: string;
}) {
  const [session, setSession] = useState<api.CloudSession | null>(null);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [stage, setStage] = useState<"form" | "code">("form");
  const [tfaKey, setTfaKey] = useState("");
  const [code, setCode] = useState("");
  const [rows, setRows] = useState<api.CloudDeviceRow[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [sent, setSent] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    api
      .cloudState()
      .then((state) => {
        if (!live) return;
        setSession(state);
        if (state.signedIn) void loadDevices();
      })
      .catch(() => undefined);
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function loadDevices() {
    try {
      setRows(await api.cloudDevices());
    } catch (err) {
      setNote(String(err));
    }
  }

  async function signInWithBambu() {
    setBusy(true);
    setNote(null);
    try {
      setSession(await api.cloudLoginWeb());
      setStage("form");
      await loadDevices();
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function signIn() {
    if (!email.trim() || !password) return;
    setBusy(true);
    setNote(null);
    try {
      const step = await api.cloudLogin(email.trim(), password);
      setPassword("");
      if (step.needsCode) {
        setTfaKey(step.tfaKey ?? "");
        setStage("code");
        setNote("Bambu sent a six-digit code to that address.");
      } else {
        setSession(await api.cloudState());
        setStage("form");
        await loadDevices();
      }
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function verify() {
    setBusy(true);
    setNote(null);
    try {
      await api.cloudVerify(email.trim(), tfaKey, code.trim());
      setCode("");
      setStage("form");
      setSession(await api.cloudState());
      await loadDevices();
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function chooseRegion(region: "Global" | "China") {
    setBusy(true);
    setNote(null);
    try {
      setSession(await api.cloudSetRegion(region));
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function signOut() {
    setBusy(true);
    try {
      await api.cloudSignOut();
      setSession(await api.cloudState());
      setRows(null);
      setStage("form");
      setNote(null);
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function send(devId: string) {
    setBusy(true);
    setNote(null);
    setSent(null);
    try {
      const id = await api.cloudSend(devId, taskName, {
        amsMapping: mapping,
        timelapse: options?.timelapse,
        bedLevelling: options?.bedLevelling,
        flowCali: options?.flowCali,
        vibrationCali: options?.vibrationCali,
        useAms: options?.useAms,
        material,
        colour,
        nozzle,
      });
      setSent(`task ${id} created`);
      toast.success(`Task ${id} created`);
      await loadDevices();
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="cloud-panel">
      <p className="cloud-kicker">Bambu Cloud</p>
      {!session ? (
        <p className="cloud-line dim">reading the account…</p>
      ) : session.signedIn ? (
        <>
          <p className="cloud-line">
            signed in as <span className="ink">{session.account ?? "an account"}</span>
          </p>
          <p className="cloud-host dim">{session.host.replace(/^https?:\/\//, "")}</p>

          {rows === null ? (
            <p className="cloud-line dim">asking the cloud for the printers…</p>
          ) : rows.length === 0 ? (
            <p className="cloud-empty">No printers on this account.</p>
          ) : (
            <ul className="cloud-devices">
              {rows.map((row) => (
                <li key={row.devId} className={row.mayPrint ? "cloud-device" : "cloud-device dim"}>
                  <span className="cloud-name">{row.label}</span>
                  <span className="cloud-model">{row.model || "—"}</span>
                  <span className="cloud-state">{row.state}</span>
                  <Button type="button" size="sm" variant="secondary" disabled={!row.mayPrint || busy} onClick={() => send(row.devId)}>
                    Send plate
                  </Button>
                </li>
              ))}
            </ul>
          )}

          {options && onOptions && <PrintOptionFields options={options} onChange={onOptions} />}

          <div className="cloud-actions">
            <Button type="button" size="sm" variant="ghost" disabled={busy} onClick={() => void loadDevices()}>
              Refresh
            </Button>
            <Button type="button" size="sm" variant="ghost" disabled={busy} onClick={() => void signOut()}>
              Sign out
            </Button>
          </div>
        </>
      ) : (
        <>
          <p className="cloud-line dim">
            The same account Bambu Studio uses: the printers in it, whether or not they are on this
            network. A China account uses api.bambulab.cn.
          </p>
          <div className="cloud-actions">
            <Button
              type="button"
              size="sm"
              variant={session.host.includes("bambulab.cn") ? "ghost" : "secondary"}
              disabled={busy}
              onClick={() => void chooseRegion("Global")}
            >
              Global
            </Button>
            <Button
              type="button"
              size="sm"
              variant={session.host.includes("bambulab.cn") ? "secondary" : "ghost"}
              disabled={busy}
              onClick={() => void chooseRegion("China")}
            >
              China
            </Button>
          </div>
          <div className="cloud-actions">
            <Button type="button" variant="secondary" disabled={busy} onClick={() => void signInWithBambu()}>
              Sign in with Bambu Lab (Google, Apple, email)
            </Button>
          </div>
          <p className="cloud-line dim">Or use email and password here.</p>
          {stage === "form" ? (
            <div className="cloud-form">
              <label>
                <span>Email</span>
                <Input
                  type="email"
                  aria-label="Email"
                  value={email}
                  autoComplete="username"
                  onChange={(event) => setEmail(event.target.value)}
                />
              </label>
              <label>
                <span>Password</span>
                <Input
                  type="password"
                  aria-label="Password"
                  value={password}
                  autoComplete="current-password"
                  onChange={(event) => setPassword(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") void signIn();
                  }}
                />
              </label>
              <Button type="button" variant="primary" disabled={busy || !email.trim() || !password} onClick={() => void signIn()}>
                Sign in
              </Button>
            </div>
          ) : (
            <div className="cloud-form">
              <label>
                <span>Code</span>
                <Input
                  inputMode="numeric"
                  aria-label="Code"
                  value={code}
                  autoComplete="one-time-code"
                  onChange={(event) => setCode(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") void verify();
                  }}
                />
              </label>
              <Button type="button" variant="primary" disabled={busy || code.trim().length < 4} onClick={() => void verify()}>
                Verify
              </Button>
              <Button
                type="button"
                variant="ghost"
                disabled={busy}
                onClick={() => {
                  setStage("form");
                  setCode("");
                  setNote(null);
                }}
              >
                Back
              </Button>
            </div>
          )}
        </>
      )}

      {note ? <p className="cloud-note">{note}</p> : null}
      {sent ? <p className="cloud-sent">{sent}</p> : null}
    </section>
  );
}
