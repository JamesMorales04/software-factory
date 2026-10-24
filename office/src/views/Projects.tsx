import { useEffect, useRef, useState } from "react";
import {
  CoreApiError,
  coreApi,
  newIdempotencyKey,
  type Project,
} from "../coreApi";
import { formatTimestamp, shortId } from "../format";

export function Projects() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [repository, setRepository] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
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
        Registration resolves one canonical local Git repository and its current
        base commit in the Rust Core. It does not grant execution authority.
      </p>
      {error && <p className="error">{error}</p>}
      {notice && <p className="notice">{notice}</p>}

      <form className="create" onSubmit={onRegister}>
        <h3>Register a local Git Project</h3>
        <label>
          Authorized local Git repository
          <input
            data-testid="project-repository"
            value={repository}
            onChange={(event) => {
              setRepository(event.target.value);
              attemptKey.current = newIdempotencyKey();
            }}
            placeholder="/absolute/path/to/local/repository"
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
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
