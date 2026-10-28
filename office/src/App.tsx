import { useCallback, useEffect, useLayoutEffect, useState } from "react";
import { Route, Routes, useLocation } from "react-router-dom";
import { AppShell } from "./components/AppShell";
import { Button } from "./components/ui/Button";
import type { AuthSession } from "./coreApi";
import {
  bootstrapOwner,
  CoreApiError,
  getAuthSession,
  logoutCurrentDevice,
  setupStatus,
} from "./coreApi";
import { ActionDetail } from "./views/ActionDetail";
import { Actions } from "./views/Actions";
import { AgentOffice } from "./views/AgentOffice";
import { CampaignDetail } from "./views/CampaignDetail";
import { Campaigns } from "./views/Campaigns";
import { DecisionInbox } from "./views/DecisionInbox";
import { Delegations } from "./views/Delegations";
import { DevicePairClaim } from "./views/DevicePairClaim";
import { Home } from "./views/Home";
import { Lead } from "./views/Lead";
import { Overview } from "./views/Overview";
import { ProjectFiles } from "./views/ProjectFiles";
import { Projects } from "./views/Projects";
import { RequestWorkspace } from "./views/RequestWorkspace";
import { Runs } from "./views/Runs";
import { Runtime } from "./views/Runtime";
import { Settings } from "./views/Settings";
import { Workers } from "./views/Workers";

function BootstrapGate({
  onComplete,
  initial,
  available = true,
}: {
  onComplete: (session: AuthSession) => void;
  initial: boolean;
  available?: boolean;
}) {
  const [code, setCode] = useState("");
  const [deviceName, setDeviceName] = useState("");
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
      <h1>{initial ? "Set up this Factory" : "Recover local access"}</h1>
      <p className="muted">
        {initial
          ? "Run ./factory open on this workstation. Enter the one-time owner confirmation it provides. It is sent once over this local connection and is never placed in the page URL."
          : available
            ? "Enter the one-time owner confirmation provided by ./factory recover. This browser cannot recover ownership by itself."
            : "This browser has no active device session. Run ./factory recover on the workstation to issue a new one-time confirmation, then reload this page."}
      </p>
      {available && (
        <form
          onSubmit={(event) => {
            event.preventDefault();
            setBusy(true);
            setError(null);
            void bootstrapOwner(code.trim(), deviceName.trim() || undefined)
              .then(onComplete)
              .catch((cause) => {
                setError(
                  cause instanceof CoreApiError
                    ? cause.message
                    : "Owner enrollment failed.",
                );
              })
              .finally(() => setBusy(false));
          }}
        >
          <label className="ui-field">
            <span className="ui-field-label">One-time owner confirmation</span>
            <input
              className="ui-input"
              type="password"
              value={code}
              onChange={(event) => setCode(event.target.value)}
              placeholder="Confirmation code"
              autoComplete="off"
              aria-label="One-time owner confirmation"
            />
          </label>
          <label className="ui-field">
            <span className="ui-field-label">This device name</span>
            <input
              className="ui-input"
              value={deviceName}
              onChange={(event) => setDeviceName(event.target.value)}
              placeholder="My workstation"
              autoComplete="off"
              maxLength={80}
            />
          </label>
          <Button
            type="submit"
            variant="primary"
            disabled={busy || code.trim() === ""}
          >
            {busy ? "Creating secure session…" : "Enroll this device"}
          </Button>
        </form>
      )}
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}

export function App() {
  const [authSession, setAuthSession] = useState<AuthSession | null>(null);
  const [setupRequired, setSetupRequired] = useState<boolean | null>(null);
  const [connectionError, setConnectionError] = useState<string | null>(null);
  const [authActionError, setAuthActionError] = useState<string | null>(null);
  const location = useLocation();

  const handlePairAuthenticated = useCallback((session: AuthSession) => {
    setSetupRequired(false);
    setAuthSession(session);
  }, []);

  useLayoutEffect(() => {
    if (location.pathname !== "/pair" || !location.search) return;
    const cleanHash = window.location.hash.split("?")[0];
    window.history.replaceState(
      null,
      "",
      `${window.location.pathname}${cleanHash}`,
    );
  }, [location.pathname, location.search]);

  useEffect(() => {
    let active = true;
    void Promise.all([setupStatus(), getAuthSession()])
      .then(([status, session]) => {
        if (active) {
          setSetupRequired(status.required);
          setAuthSession(session);
          setConnectionError(null);
        }
      })
      .catch((cause) => {
        if (active) {
          setConnectionError(
            cause instanceof CoreApiError
              ? cause.message
              : "Factory Core is unavailable.",
          );
        }
      });
    return () => {
      active = false;
    };
  }, []);

  if (setupRequired === null || connectionError !== null) {
    return (
      <div className="gate">
        <p className="muted" role={connectionError ? "alert" : undefined}>
          {connectionError ?? "Contacting Factory Core…"}
        </p>
        {connectionError && (
          <Button onClick={() => window.location.reload()}>Retry</Button>
        )}
      </div>
    );
  }
  if (setupRequired) {
    return (
      <BootstrapGate
        initial
        onComplete={(session) => {
          setSetupRequired(false);
          setAuthSession(session);
        }}
      />
    );
  }
  if (location.pathname === "/pair")
    return <DevicePairClaim onAuthenticated={handlePairAuthenticated} />;
  if (authSession === null) return null;
  if (!authSession.authenticated) {
    return (
      <BootstrapGate
        initial={false}
        available={authSession.bootstrapAvailable === true}
        onComplete={setAuthSession}
      />
    );
  }

  return (
    <AppShell
      onLock={() => {
        setAuthActionError(null);
        void logoutCurrentDevice()
          .then(() => {
            setAuthSession({ authenticated: false, bootstrapAvailable: false });
          })
          .catch((cause) => {
            setAuthActionError(
              cause instanceof CoreApiError
                ? cause.message
                : "Unable to end this device session.",
            );
          });
      }}
    >
      {authActionError && (
        <p className="ui-callout ui-callout--danger" role="alert">
          {authActionError}
        </p>
      )}
      <Routes>
        <Route path="/" element={<Home />} />
        <Route path="/requests/:requestId" element={<RequestWorkspace />} />
        <Route path="/overview" element={<Overview />} />
        <Route path="/inbox" element={<DecisionInbox />} />
        <Route path="/projects" element={<Projects />} />
        <Route path="/projects/:projectId/files" element={<ProjectFiles />} />
        <Route path="/actions" element={<Actions />} />
        <Route path="/actions/:actionId" element={<ActionDetail />} />
        <Route path="/lead" element={<Lead />} />
        <Route path="/delegations" element={<Delegations />} />
        <Route path="/campaigns" element={<Campaigns />} />
        <Route path="/campaigns/:campaignId" element={<CampaignDetail />} />
        <Route path="/agents" element={<AgentOffice />} />
        <Route path="/agents/:participantId" element={<AgentOffice />} />
        <Route path="/runs" element={<Runs />} />
        <Route path="/workers" element={<Workers />} />
        <Route path="/runtime" element={<Runtime />} />
        <Route path="/settings" element={<Settings />} />
        <Route path="*" element={<p className="error">Unknown route.</p>} />
      </Routes>
    </AppShell>
  );
}
