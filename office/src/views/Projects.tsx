import { useCallback, useEffect, useState } from "react";
import { ApiError, api, type Capability, type Project } from "../api";

const CAPABILITIES: Capability[] = [
  "repository-edit",
  "node-toolchain",
  "generic-toolchain",
  "browser",
];

const PROFILES = ["node-basic", "node-browser", "generic"];

export function Projects() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const [id, setId] = useState("");
  const [name, setName] = useState("");
  const [repositoryPath, setRepositoryPath] = useState("");
  const [baseRefs, setBaseRefs] = useState("main");
  const [qualityProfile, setQualityProfile] = useState("node-basic");
  const [workspaceScope, setWorkspaceScope] = useState(".");
  const [capabilities, setCapabilities] = useState<Capability[]>([
    "repository-edit",
    "node-toolchain",
  ]);
  const [browserEndpoints, setBrowserEndpoints] = useState(
    "http://127.0.0.1:4173",
  );
  const [browserTestCommand, setBrowserTestCommand] = useState("npm\nstart");
  const [browserReadyPath, setBrowserReadyPath] = useState("/");
  const [browserStartupTimeout, setBrowserStartupTimeout] = useState(30);

  const refresh = useCallback(async () => {
    try {
      setProjects(await api.listProjects());
    } catch (e) {
      setError(e instanceof ApiError ? e.message : "Failed to load Projects.");
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), 4000);
    return () => clearInterval(timer);
  }, [refresh]);

  function toggleCapability(value: Capability) {
    setCapabilities((current) =>
      current.includes(value)
        ? current.filter((item) => item !== value)
        : [...current, value],
    );
  }

  async function onRegister(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const project = await api.registerProject({
        id,
        name,
        repositoryPath,
        baseRefs: baseRefs
          .split(",")
          .map((ref) => ref.trim())
          .filter(Boolean),
        qualityProfile,
        workspaceScope,
        capabilities,
        browserEndpoints: capabilities.includes("browser")
          ? browserEndpoints
              .split(",")
              .map((endpoint) => endpoint.trim())
              .filter(Boolean)
          : [],
        browserTestServer: capabilities.includes("browser")
          ? {
              command: browserTestCommand
                .split("\n")
                .map((argument) => argument.trim())
                .filter(Boolean),
              readyPath: browserReadyPath,
              startupTimeoutSecs: browserStartupTimeout,
            }
          : undefined,
        executionMode: "trusted-local",
      });
      setNotice(
        `Registered ${project.id} at configuration revision ${project.configRevision}.`,
      );
      setId("");
      setName("");
      setRepositoryPath("");
      await refresh();
    } catch (err) {
      setError(
        err instanceof ApiError ? err.message : "Project registration failed.",
      );
    } finally {
      setBusy(false);
    }
  }

  async function toggleStatus(project: Project) {
    setBusy(true);
    setError(null);
    try {
      const updated = await api.updateProject(project.id, {
        status: project.status === "active" ? "paused" : "active",
      });
      setNotice(
        `${updated.id} is ${updated.status} at configuration revision ${updated.configRevision}.`,
      );
      await refresh();
    } catch (err) {
      setError(
        err instanceof ApiError ? err.message : "Project update failed.",
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <div>
      <h2>Projects</h2>
      <p className="muted">
        Registration binds one canonical local Git repository identity to a
        Quality Profile and an explicit capability grant. Execution remains
        TRUSTED_LOCAL: control-plane scoping is not OS isolation.
      </p>
      {error && <p className="error">{error}</p>}
      {notice && <p className="notice">{notice}</p>}

      <form className="create" onSubmit={onRegister}>
        <h3>Register a local Git Project</h3>
        <div className="grid-2">
          <label>
            Project ID
            <input
              data-testid="project-id"
              value={id}
              onChange={(e) => setId(e.target.value)}
              pattern="[a-z0-9_\-]+"
              placeholder="vlog"
              required
            />
          </label>
          <label>
            Display name
            <input
              data-testid="project-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              maxLength={120}
              placeholder="Vlog fixture"
              required
            />
          </label>
        </div>
        <label>
          Authorized local Git repository
          <input
            data-testid="project-repository"
            value={repositoryPath}
            onChange={(e) => setRepositoryPath(e.target.value)}
            placeholder="/absolute/path/to/local/repository"
            required
          />
        </label>
        <div className="grid-2">
          <label>
            Base refs (comma separated)
            <input
              data-testid="project-base-refs"
              value={baseRefs}
              onChange={(e) => setBaseRefs(e.target.value)}
              placeholder="main"
              required
            />
          </label>
          <label>
            Quality Profile
            <select
              data-testid="project-profile"
              value={qualityProfile}
              onChange={(e) => setQualityProfile(e.target.value)}
            >
              {PROFILES.map((profile) => (
                <option key={profile} value={profile}>
                  {profile}
                </option>
              ))}
            </select>
          </label>
        </div>
        <div className="grid-2">
          <label>
            Workspace scope
            <input
              value={workspaceScope}
              onChange={(e) => setWorkspaceScope(e.target.value)}
              placeholder="."
            />
          </label>
          <label>
            Execution mode
            <input value="trusted-local" readOnly />
          </label>
        </div>
        <fieldset className="checks">
          <legend>Capability grants</legend>
          {CAPABILITIES.map((capability) => (
            <label key={capability} className="inline">
              <input
                type="checkbox"
                checked={capabilities.includes(capability)}
                onChange={() => toggleCapability(capability)}
              />
              {capability}
            </label>
          ))}
        </fieldset>
        {capabilities.includes("browser") && (
          <>
            <label>
              Authorized loopback browser endpoints (comma separated)
              <input
                data-testid="project-browser-endpoints"
                value={browserEndpoints}
                onChange={(e) => setBrowserEndpoints(e.target.value)}
                placeholder="http://127.0.0.1:4173"
              />
            </label>
            <label>
              Browser-test start command (argv, one item per line)
              <textarea
                data-testid="project-browser-test-command"
                value={browserTestCommand}
                onChange={(e) => setBrowserTestCommand(e.target.value)}
                rows={2}
                placeholder={"npm\nstart"}
              />
            </label>
            <div className="grid-2">
              <label>
                Readiness path
                <input
                  data-testid="project-browser-ready-path"
                  value={browserReadyPath}
                  onChange={(e) => setBrowserReadyPath(e.target.value)}
                  placeholder="/"
                />
              </label>
              <label>
                Startup timeout (seconds)
                <input
                  data-testid="project-browser-timeout"
                  type="number"
                  min={5}
                  max={600}
                  value={browserStartupTimeout}
                  onChange={(e) =>
                    setBrowserStartupTimeout(Number(e.target.value))
                  }
                />
              </label>
            </div>
          </>
        )}
        <button type="submit" disabled={busy}>
          {busy ? "Registering…" : "Register Project"}
        </button>
      </form>

      <h3>Registered Projects ({projects.length})</h3>
      {projects.length === 0 ? (
        <p className="muted">No Projects registered.</p>
      ) : (
        <div className="project-grid">
          {projects.map((project) => (
            <article
              className="evidence"
              data-testid={`project-card-${project.id}`}
              key={project.id}
            >
              <header className="session-card-head">
                <strong>{project.name}</strong>
                <span className={`pill ${project.status}`}>
                  {project.status}
                </span>
              </header>
              <dl className="detail">
                <dt>ID</dt>
                <dd>
                  <code>{project.id}</code>
                </dd>
                <dt>Registration</dt>
                <dd>{project.registrationStatus}</dd>
                <dt>Repository</dt>
                <dd>
                  <code>{project.repositoryPath || "— legacy —"}</code>
                </dd>
                <dt>Identity</dt>
                <dd>
                  <code data-testid={`project-identity-${project.id}`}>
                    {project.repositoryIdentity
                      ? project.repositoryIdentity.slice(0, 16)
                      : "—"}
                  </code>
                </dd>
                <dt>Config revision</dt>
                <dd data-testid={`project-revision-${project.id}`}>
                  {project.configRevision}
                </dd>
                <dt>Quality Profile</dt>
                <dd>{project.qualityProfile}</dd>
                <dt>Capabilities</dt>
                <dd>{project.capabilities.join(", ") || "—"}</dd>
                <dt>Browser endpoints</dt>
                <dd>{project.browserEndpoints.join(", ") || "—"}</dd>
                <dt>Test server</dt>
                <dd>
                  {project.browserTestServer
                    ? `${project.browserTestServer.command.join(" ")} · ${project.browserTestServer.readyPath || "/"}`
                    : "agent-managed"}
                </dd>
                <dt>Security</dt>
                <dd>{project.securityLabel} (not isolated)</dd>
              </dl>
              {project.registrationStatus === "registered" && (
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => void toggleStatus(project)}
                >
                  {project.status === "active"
                    ? "Pause Project"
                    : "Resume Project"}
                </button>
              )}
            </article>
          ))}
        </div>
      )}
    </div>
  );
}
