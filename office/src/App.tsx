import { useEffect, useState } from "react";
import { Route, Routes, useLocation } from "react-router-dom";
import { AppShell } from "./components/AppShell";
import { Button } from "./components/ui/Button";
import {
  CoreApiError,
  clearToken,
  enrollOwner,
  getToken,
  setToken,
  setupStatus,
} from "./coreApi";
import { ActionDetail } from "./views/ActionDetail";
import { Actions } from "./views/Actions";
import { CampaignDetail } from "./views/CampaignDetail";
import { Campaigns } from "./views/Campaigns";
import { DecisionInbox } from "./views/DecisionInbox";
import { Delegations } from "./views/Delegations";
import { Lead } from "./views/Lead";
import { Overview } from "./views/Overview";
import { Projects } from "./views/Projects";
import { Runs } from "./views/Runs";
import { Runtime } from "./views/Runtime";
import { Settings } from "./views/Settings";
import { Workers } from "./views/Workers";

function TokenGate({ onUnlock }: { onUnlock: () => void }) {
  const [value, setValue] = useState("");
  return (
    <div className="gate">
      <div className="gate-brand">
        <span className="brand-mark" aria-hidden="true">
          SF
        </span>
        <span>
          <strong>Factory</strong>
          <span>Office</span>
        </span>
      </div>
      <h1>Unlock Factory Office</h1>
      <p className="muted">
        Paste the operator token from the private{" "}
        <code>/config/operator.token</code> file. It stays in this tab's session
        storage only.
      </p>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          setToken(value.trim());
          onUnlock();
        }}
      >
        <label className="ui-field">
          <span className="ui-field-label">Operator token</span>
          <input
            className="ui-input"
            type="password"
            value={value}
            onChange={(event) => setValue(event.target.value)}
            placeholder="FACTORY_OPERATOR_TOKEN"
            autoComplete="off"
            aria-label="Operator token"
          />
        </label>
        <Button type="submit" variant="primary" disabled={value.trim() === ""}>
          Unlock
        </Button>
      </form>
    </div>
  );
}

function SetupWizard({ onComplete }: { onComplete: () => void }) {
  const [code, setCode] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  return (
    <div className="gate">
      <div className="gate-brand">
        <span className="brand-mark" aria-hidden="true">
          SF
        </span>
        <span>
          <strong>Factory</strong>
          <span>Office</span>
        </span>
      </div>
      <h1>First-run setup</h1>
      <p className="muted">
        Factory generated a one-time enrollment code. Read it with{" "}
        <code>podman compose exec core cat /data/enrollment-code</code>, then
        enter it here. The code is invalidated after one successful use.
      </p>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          setBusy(true);
          setError(null);
          void enrollOwner(code.trim())
            .then((token) => {
              setToken(token);
              onComplete();
            })
            .catch((cause) => {
              setError(
                cause instanceof CoreApiError
                  ? cause.message
                  : "First-run setup failed.",
              );
            })
            .finally(() => setBusy(false));
        }}
      >
        <label className="ui-field">
          <span className="ui-field-label">One-time enrollment code</span>
          <input
            className="ui-input"
            type="password"
            value={code}
            onChange={(event) => setCode(event.target.value)}
            placeholder="One-time enrollment code"
            autoComplete="off"
            aria-label="One-time enrollment code"
          />
        </label>
        <Button
          type="submit"
          variant="primary"
          disabled={busy || code.trim() === ""}
        >
          {busy ? "Enrolling…" : "Enroll owner"}
        </Button>
      </form>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}

export function App() {
  const [unlocked, setUnlocked] = useState(() => getToken() !== null);
  const [setupRequired, setSetupRequired] = useState<boolean | null>(null);
  const location = useLocation();

  useEffect(() => {
    let active = true;
    void setupStatus()
      .then((status) => {
        if (active) setSetupRequired(status.required);
      })
      .catch(() => {
        if (active) setSetupRequired(false);
      });
    return () => {
      active = false;
    };
  }, []);

  if (setupRequired === null) {
    return (
      <div className="gate">
        <p className="muted">Contacting Factory Core…</p>
      </div>
    );
  }
  if (setupRequired) {
    return (
      <SetupWizard
        onComplete={() => {
          setSetupRequired(false);
          setUnlocked(true);
        }}
      />
    );
  }
  if (!unlocked) {
    return <TokenGate onUnlock={() => setUnlocked(true)} />;
  }

  return (
    <AppShell
      pathname={location.pathname}
      onLock={() => {
        clearToken();
        setUnlocked(false);
      }}
    >
      <Routes>
        <Route path="/" element={<Overview />} />
        <Route path="/inbox" element={<DecisionInbox />} />
        <Route path="/projects" element={<Projects />} />
        <Route path="/actions" element={<Actions />} />
        <Route path="/actions/:actionId" element={<ActionDetail />} />
        <Route path="/lead" element={<Lead />} />
        <Route path="/delegations" element={<Delegations />} />
        <Route path="/campaigns" element={<Campaigns />} />
        <Route path="/campaigns/:campaignId" element={<CampaignDetail />} />
        <Route path="/runs" element={<Runs />} />
        <Route path="/workers" element={<Workers />} />
        <Route path="/runtime" element={<Runtime />} />
        <Route path="/settings" element={<Settings />} />
        <Route path="*" element={<p className="error">Unknown route.</p>} />
      </Routes>
    </AppShell>
  );
}
