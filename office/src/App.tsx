import { useState } from "react";
import { Link, NavLink, Route, Routes } from "react-router-dom";
import { clearToken, getToken, setToken } from "./api";
import { ActionDetail } from "./views/ActionDetail";
import { Actions } from "./views/Actions";
import { Agents } from "./views/Agents";
import { CampaignDetail } from "./views/CampaignDetail";
import { Campaigns } from "./views/Campaigns";
import { Dashboard } from "./views/Dashboard";
import { Native } from "./views/Native";
import { Projects } from "./views/Projects";
import { RunDetail } from "./views/RunDetail";
import { Runs } from "./views/Runs";
import { Workers } from "./views/Workers";

function TokenGate({ onUnlock }: { onUnlock: () => void }) {
  const [value, setValue] = useState("");
  return (
    <div className="gate">
      <h2>Factory Office</h2>
      <p className="muted">
        The Core requires a Bearer token. Paste the value from the private{" "}
        <code>api.token</code> file reported by <code>factory start</code>, or
        the token configured for a direct Core run. It stays in this tab's
        session storage only.
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
          placeholder="FACTORY_API_TOKEN"
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
          <span className="muted">S4-C</span>
        </h1>
        <nav>
          <NavLink
            to="/"
            end
            className={({ isActive }) =>
              isActive ? "navlink active" : "navlink"
            }
          >
            Dashboard
          </NavLink>
          <NavLink
            to="/projects"
            end
            className={({ isActive }) =>
              isActive ? "navlink active" : "navlink"
            }
          >
            Projects
          </NavLink>
          <NavLink
            to="/actions"
            end
            className={({ isActive }) =>
              isActive ? "navlink active" : "navlink"
            }
          >
            Actions
          </NavLink>
          <NavLink
            to="/campaigns"
            end
            className={({ isActive }) =>
              isActive ? "navlink active" : "navlink"
            }
          >
            Campaigns
          </NavLink>
          <NavLink
            to="/runs"
            end
            className={({ isActive }) =>
              isActive ? "navlink active" : "navlink"
            }
          >
            Runs
          </NavLink>
          <NavLink
            to="/workers"
            end
            className={({ isActive }) =>
              isActive ? "navlink active" : "navlink"
            }
          >
            Workers
          </NavLink>
          <NavLink
            to="/agents"
            end
            className={({ isActive }) =>
              isActive ? "navlink active" : "navlink"
            }
          >
            Agents
          </NavLink>
          <NavLink
            to="/native"
            end
            className={({ isActive }) =>
              isActive ? "navlink active" : "navlink"
            }
          >
            Native
          </NavLink>
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
          <Route path="/actions/:id" element={<ActionDetail />} />
          <Route path="/campaigns" element={<Campaigns />} />
          <Route path="/campaigns/:id" element={<CampaignDetail />} />
          <Route path="/runs" element={<Runs />} />
          <Route path="/runs/:id" element={<RunDetail />} />
          <Route path="/workers" element={<Workers />} />
          <Route path="/agents" element={<Agents />} />
          <Route path="/native" element={<Native />} />
          <Route path="*" element={<p className="error">Unknown route.</p>} />
        </Routes>
      </main>
    </div>
  );
}
