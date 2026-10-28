import { useEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";
import {
  CoreApiError,
  coreApi,
  newIdempotencyKey,
  type Project,
  type ProjectConfig,
} from "../coreApi";
import { formatTimestamp, shortId } from "../format";

export function Projects() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [repository, setRepository] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [selected, setSelected] = useState<Project | null>(null);
  const [config, setConfig] = useState<ProjectConfig | null>(null);
  const [configPaths, setConfigPaths] = useState("");
  const [configCapabilities, setConfigCapabilities] = useState<string[]>([]);
  const [qualityProfile, setQualityProfile] = useState("generic");
  const [active, setActive] = useState(true);
  const attemptKey = useRef(newIdempotencyKey());

  useEffect(() => {
    let active = true;
    async function load() {
      try {
        const next = await coreApi.listProjects();
        if (active) {
          setProjects(next);
        }
      } catch (cause) {
        if (active) {
          setError(
            cause instanceof CoreApiError
              ? cause.message
              : "Failed to load Projects.",
          );
        }
      }
    }
    void load();
    const timer = setInterval(() => void load(), 4000);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, []);

  async function onRegister(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const project = await coreApi.registerProject({
        repository: repository.trim(),
        idempotency_key: attemptKey.current,
      });
      attemptKey.current = newIdempotencyKey();
      setNotice(
        `Registered ${shortId(project.id)} at base commit ${shortId(project.base_commit, 12)}.`,
      );
      setRepository("");
      const next = await coreApi.listProjects();
      setProjects(next);
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : "Project registration failed.",
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <div>
      <h2>Projects</h2>
      <p className="muted">
        In the container deployment, register an HTTPS Git URL. Core imports it
        into Factory-managed storage and freezes the current base commit.
        Registration does not grant execution authority.
      </p>
      {error && <p className="error">{error}</p>}
      {notice && <p className="notice">{notice}</p>}

      <form className="create" onSubmit={onRegister}>
        <h3>Register a Git Project</h3>
        <label>
          HTTPS Git repository URL (or managed path)
          <input
            data-testid="project-repository"
            value={repository}
            onChange={(event) => {
              setRepository(event.target.value);
              attemptKey.current = newIdempotencyKey();
            }}
            placeholder="https://github.com/owner/repository.git"
            required
          />
        </label>
        <button type="submit" disabled={busy || repository.trim() === ""}>
          {busy ? "Registering…" : "Register Project"}
        </button>
      </form>

      <h3>Registered Projects ({projects.length})</h3>
      {projects.length === 0 ? (
        <p className="muted">No Projects registered.</p>
      ) : (
        <table>
          <thead>
            <tr>
              <th>ID</th>
              <th>Repository</th>
              <th>Base commit</th>
              <th>Created</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {projects.map((project) => (
              <tr key={project.id}>
                <td>
                  <code>{shortId(project.id)}</code>
                </td>
                <td>
                  <code>{project.repository}</code>
                </td>
                <td>
                  <code title={project.base_commit}>
                    {shortId(project.base_commit, 12)}
                  </code>
                </td>
                <td className="muted">{formatTimestamp(project.created_at)}</td>
                <td className="project-actions">
                  <Link
                    className="ui-button ui-button--secondary ui-button--sm"
                    to={`/projects/${encodeURIComponent(project.id)}/files`}
                  >
                    Browse files
                  </Link>
                  <button
                    type="button"
                    onClick={() => {
                      setSelected(project);
                      setError(null);
                      void coreApi
                        .getProjectConfig(project.id)
                        .then((next) => {
                          setConfig(next);
                          setConfigPaths(next.allowedPaths.join(", "));
                          setConfigCapabilities(next.capabilities);
                          setQualityProfile(next.qualityProfile);
                          setActive(next.active);
                        })
                        .catch((cause) =>
                          setError(
                            cause instanceof CoreApiError
                              ? cause.message
                              : "Project config could not be loaded.",
                          ),
                        );
                    }}
                  >
                    Configure
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {selected && config && (
        <form
          className="create"
          onSubmit={(event) => {
            event.preventDefault();
            setBusy(true);
            setError(null);
            void coreApi
              .updateProjectConfig(selected.id, {
                expectedRevision: config.revision,
                allowedPaths: configPaths
                  .split(",")
                  .map((path) => path.trim())
                  .filter(Boolean),
                capabilities: configCapabilities,
                qualityProfile,
                active,
              })
              .then((next) => {
                setConfig(next);
                setNotice(
                  `Project config revision ${next.revision} saved (${next.configHash.slice(0, 12)}).`,
                );
              })
              .catch((cause) =>
                setError(
                  cause instanceof CoreApiError
                    ? cause.message
                    : "Project config update failed.",
                ),
              )
              .finally(() => setBusy(false));
          }}
        >
          <h3>Project configuration — {shortId(selected.id)}</h3>
          <p className="muted">
            Revision {config.revision} · base{" "}
            <code>{shortId(config.baseCommit, 12)}</code> · hash{" "}
            <code>{shortId(config.configHash, 12)}</code>
          </p>
          <label>
            Allowed repository paths (comma separated)
            <input
              value={configPaths}
              onChange={(event) => setConfigPaths(event.target.value)}
              placeholder="src, tests"
            />
          </label>
          <fieldset>
            <legend>Capabilities</legend>
            {[
              "repository-edit",
              "generic-toolchain",
              "node-toolchain",
              "browser",
            ].map((capability) => (
              <label className="inline" key={capability}>
                <input
                  type="checkbox"
                  checked={configCapabilities.includes(capability)}
                  onChange={(event) =>
                    setConfigCapabilities((current) =>
                      event.target.checked
                        ? [...current, capability]
                        : current.filter((item) => item !== capability),
                    )
                  }
                />
                {capability}
              </label>
            ))}
          </fieldset>
          <div className="grid-2">
            <label>
              Quality profile
              <select
                value={qualityProfile}
                onChange={(event) => setQualityProfile(event.target.value)}
              >
                <option value="generic">generic</option>
                <option value="node-basic">node-basic</option>
                <option value="node-browser">node-browser</option>
              </select>
            </label>
            <label className="inline">
              <input
                type="checkbox"
                checked={active}
                onChange={(event) => setActive(event.target.checked)}
              />
              Active
            </label>
          </div>
          <button type="submit" disabled={busy}>
            {busy ? "Saving…" : "Save project config"}
          </button>
        </form>
      )}
    </div>
  );
}
