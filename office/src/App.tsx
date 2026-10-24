import { useState } from "react";
import { Link, NavLink, Route, Routes } from "react-router-dom";
import { clearToken, getToken, setToken } from "./coreApi";
import { Actions } from "./views/Actions";
import { Campaigns } from "./views/Campaigns";
import { Dashboard } from "./views/Dashboard";
import { Lead } from "./views/Lead";
import { Projects } from "./views/Projects";
import { Runs } from "./views/Runs";
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
        The Core requires a Bearer token. Paste the operator token from the
        private <code>api.token</code> file reported by{" "}
        <code>factory start</code>, or the token configured for a direct Core
        run. It stays in this tab's session storage only.
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

export function App() {
  const [unlocked, setUnlocked] = useState(() => getToken() !== null);

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
          <NavItem to="/campaigns" label="Campaigns" />
          <NavItem to="/runs" label="Runs" />
          <NavItem to="/workers" label="Workers" />
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
          <Route path="/campaigns" element={<Campaigns />} />
          <Route path="/runs" element={<Runs />} />
          <Route path="/workers" element={<Workers />} />
          <Route path="*" element={<p className="error">Unknown route.</p>} />
        </Routes>
      </main>
    </div>
  );
}
