import { useEffect, useState } from "react";
import { Link, NavLink, Route, Routes } from "react-router-dom";
import {
  CoreApiError,
  clearToken,
  enrollOwner,
  getToken,
  setToken,
  setupStatus,
} from "./coreApi";
import { Actions } from "./views/Actions";
import { CampaignDetail } from "./views/CampaignDetail";
import { Campaigns } from "./views/Campaigns";
import { Dashboard } from "./views/Dashboard";
import { Delegations } from "./views/Delegations";
import { Lead } from "./views/Lead";
import { Projects } from "./views/Projects";
import { Runs } from "./views/Runs";
import { Runtime } from "./views/Runtime";
import { Settings } from "./views/Settings";
import { Workers } from "./views/Workers";

function NavItem({ to, label }: { to: string; label: string }) {
  return (
    <NavLink
      to={to}
      end
      className={({ isActive }) => (isActive ? "navlink active" : "navlink")}
    >
      {label}
    </NavLink>
  );
}

function TokenGate({ onUnlock }: { onUnlock: () => void }) {
  const [value, setValue] = useState("");
  return (
    <div className="gate">
      <h2>Factory Office</h2>
      <p className="muted">
        Paste the operator token from the private{" "}
        <code>/config/operator.token</code> file (read it with{" "}
        <code>podman compose exec core cat /config/operator.token</code>). It
        stays in this tab's session storage only.
      </p>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          setToken(value.trim());
          onUnlock();
        }}
      >
        <input
          type="password"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          placeholder="FACTORY_OPERATOR_TOKEN"
          autoComplete="off"
        />
        <button type="submit" disabled={value.trim() === ""}>
          Unlock
        </button>
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
      <h2>Factory first-run setup</h2>
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
        <input
          type="password"
          value={code}
          onChange={(event) => setCode(event.target.value)}
          placeholder="one-time enrollment code"
          autoComplete="off"
        />
        <button type="submit" disabled={busy || code.trim() === ""}>
          {busy ? "Enrolling…" : "Enroll owner"}
        </button>
      </form>
      {error && <p className="error">{error}</p>}
    </div>
  );
}

export function App() {
  const [unlocked, setUnlocked] = useState(() => getToken() !== null);
  const [setupRequired, setSetupRequired] = useState<boolean | null>(null);

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
    return <p className="muted">Contacting Factory Core…</p>;
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
    <div className="app">
      <header>
        <h1>
          <Link to="/" className="brand">
            Factory Office
          </Link>{" "}
          <span className="muted">Rust Core</span>
        </h1>
        <nav>
          <NavItem to="/" label="Dashboard" />
          <NavItem to="/projects" label="Projects" />
          <NavItem to="/actions" label="Actions" />
          <NavItem to="/lead" label="Lead" />
          <NavItem to="/delegations" label="Delegations" />
          <NavItem to="/campaigns" label="Campaigns" />
          <NavItem to="/runs" label="Runs" />
          <NavItem to="/workers" label="Workers" />
          <NavItem to="/runtime" label="Runtime" />
          <NavItem to="/settings" label="Settings" />
          <button
            type="button"
            className="linklike"
            onClick={() => {
              clearToken();
              setUnlocked(false);
            }}
          >
            Lock
          </button>
        </nav>
      </header>
      <main>
        <Routes>
          <Route path="/" element={<Dashboard />} />
          <Route path="/projects" element={<Projects />} />
          <Route path="/actions" element={<Actions />} />
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
      </main>
    </div>
  );
}
