import { useCallback, useEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";
import {
  type Action,
  ApiError,
  api,
  newIdempotencyKey,
  type Project,
} from "../api";

export function Actions() {
  const [actions, setActions] = useState<Action[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [projectId, setProjectId] = useState("");
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  // One idempotency key per create attempt: retries reuse it, a fresh
  // submit mints a new one. A duplicate response is reported, not hidden.
  const attemptKey = useRef<string>(newIdempotencyKey());

  const refresh = useCallback(async (signal?: AbortSignal) => {
    try {
      const [list, nextProjects] = await Promise.all([
        api.listActions(),
        api.listProjects(),
      ]);
      if (!signal?.aborted) {
        setActions(list);
        setProjects(nextProjects);
        setProjectId((current) => {
          if (current) return current;
          const eligible = nextProjects.find(
            (project) => project.registrationStatus === "registered",
          );
          return eligible?.id ?? nextProjects[0]?.id ?? "";
        });
      }
    } catch (e) {
      if (!signal?.aborted) {
        setError(e instanceof ApiError ? e.message : "Failed to load actions.");
      }
    }
  }, []);

  useEffect(() => {
    const controller = new AbortController();
    void refresh(controller.signal);
    const timer = setInterval(() => void refresh(controller.signal), 3000);
    return () => {
      controller.abort();
      clearInterval(timer);
    };
  }, [refresh]);

  async function onSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setNotice(null);
    setCreating(true);
    try {
      const { data, replayed } = await api.createAction(
        { projectId, title, description: description || undefined },
        attemptKey.current,
      );
      setNotice(
        replayed
          ? `Duplicate suppressed: this attempt already created ${data.action.id.slice(0, 8)}.`
          : `Created ${data.action.id.slice(0, 8)}.`,
      );
      setTitle("");
      setDescription("");
      attemptKey.current = newIdempotencyKey();
      await refresh();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Create failed.");
    } finally {
      setCreating(false);
    }
  }

  return (
    <div>
      <h2>Actions</h2>
      {error && <p className="error">{error}</p>}
      {notice && <p className="notice">{notice}</p>}
      <form className="create" onSubmit={onSubmit}>
        <h3>New Action</h3>
        <label>
          Project
          <select
            data-testid="action-project"
            value={projectId}
            onChange={(e) => setProjectId(e.target.value)}
            required
          >
            <option value="">Select a registered Project…</option>
            {(projects.some(
              (project) => project.registrationStatus === "registered",
            )
              ? projects.filter(
                  (project) => project.registrationStatus === "registered",
                )
              : projects
            ).map((project) => (
              <option key={project.id} value={project.id}>
                {project.name} ({project.id})
                {project.registrationStatus === "legacy" ? " — legacy" : ""}
              </option>
            ))}
          </select>
          {!projects.some(
            (project) => project.registrationStatus === "registered",
          ) && (
            <span className="muted">
              No registered Project exists. The legacy default Project can hold
              Actions for historical compatibility but cannot freeze a new
              Project-bound ActionSpec.
            </span>
          )}
        </label>
        <label>
          Title
          <input
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            maxLength={200}
            required
            placeholder="What should the Factory do?"
          />
        </label>
        <label>
          Description
          <textarea
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            maxLength={2000}
            rows={3}
            placeholder="Acceptance sketch, context, constraints…"
          />
        </label>
        <button
          type="submit"
          disabled={creating || title.trim() === "" || projectId === ""}
        >
          {creating ? "Creating…" : "Create Action"}
        </button>
      </form>
      <h3>All Actions</h3>
      {actions.length === 0 ? (
        <p className="muted">No Actions yet.</p>
      ) : (
        <table>
          <thead>
            <tr>
              <th>ID</th>
              <th>Project</th>
              <th>Title</th>
              <th>Status</th>
              <th>Updated</th>
            </tr>
          </thead>
          <tbody>
            {actions.map((a) => (
              <tr key={a.id}>
                <td>
                  <code>{a.id.slice(0, 8)}</code>
                </td>
                <td>
                  <code>{a.projectId}</code>
                </td>
                <td>
                  <Link to={`/actions/${a.id}`}>{a.title}</Link>
                </td>
                <td>
                  <span className={`pill ${a.status}`}>{a.status}</span>
                </td>
                <td className="muted">
                  {new Date(a.updatedAt).toLocaleString()}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
