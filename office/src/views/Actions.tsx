import { useEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";
import {
  type Action,
  CoreApiError,
  coreApi,
  newIdempotencyKey,
  type Project,
  type SpecCheck,
} from "../coreApi";
import { formatTimestamp, shortId } from "../format";

const FIXTURE_CHECK: SpecCheck = {
  id: "content",
  kind: "functional",
  description: "fixture content",
  command: ["grep", "-q", "fixture candidate", "src/fixture.txt"],
  expectExit: 0,
  timeoutSecs: 30,
};

export function Actions() {
  const [actions, setActions] = useState<Action[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [projectId, setProjectId] = useState("");
  const [objective, setObjective] = useState("");
  const [provider, setProvider] = useState("opencode");
  const [model, setModel] = useState("test/model");
  const [allowedPaths, setAllowedPaths] = useState("src");
  const [fixtureCheck, setFixtureCheck] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const attemptKey = useRef(newIdempotencyKey());

  useEffect(() => {
    let active = true;
    async function load() {
      try {
        const [nextActions, nextProjects] = await Promise.all([
          coreApi.listActions(),
          coreApi.listProjects(),
        ]);
        if (!active) {
          return;
        }
        setActions(nextActions);
        setProjects(nextProjects);
        setProjectId((current) => current || nextProjects[0]?.id || "");
      } catch (cause) {
        if (active) {
          setError(
            cause instanceof CoreApiError
              ? cause.message
              : "Failed to load Actions.",
          );
        }
      }
    }
    void load();
    const timer = setInterval(() => void load(), 3000);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, []);

  function resetKey() {
    attemptKey.current = newIdempotencyKey();
  }

  async function onCreate(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const action = await coreApi.createAction({
        project_id: projectId,
        objective: objective.trim(),
        provider,
        model: model.trim(),
        idempotency_key: attemptKey.current,
        allowed_paths: allowedPaths
          .split(",")
          .map((path) => path.trim())
          .filter(Boolean),
        checks: fixtureCheck ? [FIXTURE_CHECK] : [],
      });
      attemptKey.current = newIdempotencyKey();
      setNotice(
        `Created Action ${shortId(action.id)} with spec ${shortId(action.spec_sha256, 12)}.`,
      );
      setObjective("");
      const next = await coreApi.listActions();
      setActions(next);
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : "Action creation failed.",
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <div>
      <h2>Actions</h2>
      <p className="muted">
        An Action freezes the objective, model route, allowed paths and checks
        that later authorizations and quality decisions bind to.
      </p>
      {error && <p className="error">{error}</p>}
      {notice && <p className="notice">{notice}</p>}

      {projects.length === 0 ? (
        <p className="notice">
          Register a Project before creating an Action.{" "}
          <Link to="/projects">Open Projects</Link>
        </p>
      ) : (
        <form className="create" onSubmit={onCreate}>
          <h3>New Action</h3>
          <label>
            Project
            <select
              data-testid="action-project"
              value={projectId}
              onChange={(event) => {
                setProjectId(event.target.value);
                resetKey();
              }}
              required
            >
              {projects.map((project) => (
                <option key={project.id} value={project.id}>
                  {project.repository} ({shortId(project.id)})
                </option>
              ))}
            </select>
          </label>
          <label>
            Objective
            <textarea
              data-testid="action-objective"
              value={objective}
              onChange={(event) => {
                setObjective(event.target.value);
                resetKey();
              }}
              maxLength={2000}
              rows={3}
              required
              placeholder="What should the Factory do?"
            />
          </label>
          <div className="grid-2">
            <label>
              Provider
              <select
                value={provider}
                onChange={(event) => {
                  setProvider(event.target.value);
                  resetKey();
                }}
              >
                <option value="opencode">opencode</option>
                <option value="codex">codex</option>
              </select>
            </label>
            <label>
              Model
              <input
                value={model}
                onChange={(event) => {
                  setModel(event.target.value);
                  resetKey();
                }}
                maxLength={128}
                required
                placeholder="test/model"
              />
            </label>
          </div>
          <label>
            Allowed repository paths (comma separated)
            <input
              value={allowedPaths}
              onChange={(event) => {
                setAllowedPaths(event.target.value);
                resetKey();
              }}
              placeholder="src"
            />
          </label>
          <label className="inline">
            <input
              type="checkbox"
              checked={fixtureCheck}
              onChange={(event) => {
                setFixtureCheck(event.target.checked);
                resetKey();
              }}
            />
            Include the deterministic fixture content check
          </label>
          <button type="submit" disabled={busy || objective.trim() === ""}>
            {busy ? "Creating…" : "Create Action"}
          </button>
        </form>
      )}

      <h3>All Actions ({actions.length})</h3>
      {actions.length === 0 ? (
        <p className="muted">No Actions yet.</p>
      ) : (
        <table>
          <thead>
            <tr>
              <th>ID</th>
              <th>Project</th>
              <th>Objective</th>
              <th>Route</th>
              <th>Risk</th>
              <th>Status</th>
              <th>Spec</th>
              <th>Created</th>
            </tr>
          </thead>
          <tbody>
            {actions.map((action) => (
              <tr key={action.id}>
                <td>
                  <code>{shortId(action.id)}</code>
                </td>
                <td>
                  <code>{shortId(action.project_id)}</code>
                </td>
                <td>
                  {action.objective}
                  <div className="muted">
                    paths {action.spec.allowed_paths.join(", ") || "—"}
                  </div>
                </td>
                <td>
                  <code>
                    {action.provider}/{action.model}
                  </code>
                </td>
                <td>{action.risk}</td>
                <td>
                  <span className={`pill ${action.status.toLowerCase()}`}>
                    {action.status}
                  </span>
                </td>
                <td>
                  <code title={action.spec_sha256}>
                    {shortId(action.spec_sha256, 12)}
                  </code>
                </td>
                <td className="muted">{formatTimestamp(action.created_at)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
