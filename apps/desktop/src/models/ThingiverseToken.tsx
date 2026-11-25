import { useEffect, useState } from "react";
import { api } from "../api";
import { Button, Input } from "../ui";

const THINGIVERSE_DEVELOPERS = "https://www.thingiverse.com/developers";

/** Settings field for the user's own Thingiverse app token. */
export function ThingiverseToken() {
  const [configured, setConfigured] = useState(false);
  const [draft, setDraft] = useState("");
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    api
      .thingiverseTokenStatus()
      .then((status) => {
        if (live) setConfigured(status.configured);
      })
      .catch(() => {
        if (live) setConfigured(false);
      });
    return () => {
      live = false;
    };
  }, []);

  async function save() {
    setMessage(null);
    try {
      const status = await api.thingiverseTokenSet(draft);
      setConfigured(status.configured);
      setDraft("");
      setMessage("Token saved on this computer.");
    } catch (err) {
      setMessage(err instanceof Error ? err.message : String(err));
    }
  }

  async function clear() {
    setMessage(null);
    try {
      const status = await api.thingiverseTokenClear();
      setConfigured(status.configured);
      setMessage("Token removed.");
    } catch (err) {
      setMessage(err instanceof Error ? err.message : String(err));
    }
  }

  return (
    <div className="settings-group">
      <p className="hint">
        Thingiverse search uses an app token you create. Open the{" "}
        <button
          type="button"
          className="models-text-button"
          onClick={() => void api.openModelLink(THINGIVERSE_DEVELOPERS)}
        >
          Thingiverse developer page
        </button>
        , make a desktop app, and paste the token. Shale never includes one of its own.
      </p>
      <p className="hint">{configured ? "A token is saved on this computer." : "No token saved yet."}</p>
      <label className="ui-field">
        <span className="ui-field__label">App token</span>
        <Input
          type="password"
          autoComplete="off"
          value={draft}
          aria-label="Thingiverse app token"
          placeholder="Paste the app token"
          onChange={(event) => setDraft(event.target.value)}
        />
      </label>
      <div className="models-actions">
        <Button type="button" variant="primary" onClick={() => void save()}>
          Save token
        </Button>
        {configured ? (
          <Button type="button" variant="ghost" onClick={() => void clear()}>
            Remove token
          </Button>
        ) : null}
      </div>
      {message ? <p className="hint">{message}</p> : null}
    </div>
  );
}
