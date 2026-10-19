import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { ApiError, api, newIdempotencyKey, type Run } from "../api";

// Runs view: real Core Run projections — queue a permitted synthetic Run,
// watch its lifecycle, inspect Attempts and cancel when needed. Nothing
// here is fabricated: every row comes from an authoritative Core snapshot.
export function Runs() {
  const [runs, setRuns] = useState<Run[]>([]);
  const [message, setMessage] = useState("hello-factory");
  const [steps, setSteps] = useState("2");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const refresh = useCallback(async (signal?: AbortSignal) => {
    try {
      const list = await api.listRuns();
      if (!signal?.aborted) {
        setRuns(list);
      }
    } catch (e) {
      if (!signal?.aborted) {
        setError(e instanceof ApiError ? e.message : "Failed to load runs.");
      }
    }
  }, []);

  useEffect(() => {
    const controller = new AbortController();
    void refresh(controller.signal);
    const timer = setInterval(() => void refresh(controller.signal), 2000);
    return () => {
      controller.abort();
      clearInterval(timer);
    };
  }, [refresh]);

  async function onSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setNotice(null);
    setSubmitting(true);
    try {
      const parsedSteps = Math.max(
        1,
        Math.min(10, Number.parseInt(steps, 10) || 1),
      );
      const { data } = await api.createRun(
        { message: message.trim(), steps: parsedSteps, stepMs: 5 },
        newIdempotencyKey(),
      );
      setNotice(
        `Queued Run ${data.run.id.slice(0, 8)} — watch it lease, start and complete below.`,
      );
      await refresh();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Queue failed.");
    } finally {
      setSubmitting(false);
    }
  }

  async function onCancel(id: string) {
    setError(null);
    try {
      await api.cancelRun(id);
      await refresh();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Cancel failed.");
    }
  }

  return (
    <div>
      <h2>Runs</h2>
      {error && <p className="error">{error}</p>}
      {notice && <p className="notice">{notice}</p>}
      <form className="create" onSubmit={onSubmit}>
        <h3>New synthetic Run (credential-free fixture)</h3>
        <label>
          Message (≤256 chars)
          <input
            value={message}
            onChange={(e) => setMessage(e.target.value)}
            maxLength={256}
            required
            placeholder="short marker echoed by the fixture"
          />
        </label>
        <label>
          Steps (1–10)
          <input
            value={steps}
            onChange={(e) => setSteps(e.target.value)}
            inputMode="numeric"
            pattern="[0-9]*"
          />
        </label>
        <button type="submit" disabled={submitting || message.trim() === ""}>
          {submitting ? "Queueing…" : "Queue Run"}
        </button>
      </form>
      <h3>All Runs</h3>
      {runs.length === 0 ? (
        <p className="muted">
          No Runs yet — queue one above to see Core–Worker dispatch.
        </p>
      ) : (
        <table>
          <thead>
            <tr>
              <th>ID</th>
              <th>Status</th>
              <th>Attempts</th>
              <th>Worker</th>
              <th>Updated</th>
              <th>Action</th>
            </tr>
          </thead>
          <tbody>
            {runs.map((r) => (
              <tr key={r.id}>
                <td>
                  <Link to={`/runs/${r.id}`}>
                    <code>{r.id.slice(0, 8)}</code>
                  </Link>
                </td>
                <td>
                  <span className={`pill ${r.status}`}>{r.status}</span>
                </td>
                <td>
                  {r.attemptCount}/{r.maxAttempts}
                </td>
                <td className="muted">{r.leaseOwner || "—"}</td>
                <td className="muted">
                  {new Date(r.updatedAt).toLocaleString()}
                </td>
                <td>
                  {!["succeeded", "failed", "cancelled"].includes(r.status) && (
                    <button
                      type="button"
                      className="linklike"
                      onClick={() => void onCancel(r.id)}
                    >
                      Cancel
                    </button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
