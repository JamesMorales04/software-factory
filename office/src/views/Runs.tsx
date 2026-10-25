import { useEffect, useState } from "react";
import { type Attempt, CoreApiError, coreApi, type Run } from "../coreApi";
import { formatTimestamp, shortId } from "../format";

export function Runs() {
  const [runs, setRuns] = useState<Run[]>([]);
  const [attempts, setAttempts] = useState<Attempt[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    async function load() {
      try {
        const [nextRuns, nextAttempts] = await Promise.all([
          coreApi.listRuns(),
          coreApi.listAttempts(),
        ]);
        if (active) {
          setRuns(nextRuns);
          setAttempts(nextAttempts);
        }
      } catch (cause) {
        if (active) {
          setError(
            cause instanceof CoreApiError
              ? cause.message
              : "Failed to load Runs.",
          );
        }
      }
    }
    void load();
    const timer = setInterval(() => void load(), 2000);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, []);

  async function onQueueAttempt(run: Run) {
    setBusyId(run.id);
    setError(null);
    setNotice(null);
    try {
      const attempt = await coreApi.createRunAttempt(run.id);
      setNotice(
        `Queued Attempt ${shortId(attempt.id)} for Run ${shortId(run.id)}.`,
      );
      const [nextRuns, nextAttempts] = await Promise.all([
        coreApi.listRuns(),
        coreApi.listAttempts(),
      ]);
      setRuns(nextRuns);
      setAttempts(nextAttempts);
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : "Attempt queueing failed.",
      );
    } finally {
      setBusyId(null);
    }
  }

  const attemptsByRun = new Map<string, Attempt[]>();
  for (const attempt of attempts) {
    const list = attemptsByRun.get(attempt.run_id) ?? [];
    list.push(attempt);
    attemptsByRun.set(attempt.run_id, list);
  }

  return (
    <div>
      <h2>Runs</h2>
      <p className="muted">
        Runs are created from an authorized Campaign. Queue one Attempt per
        queued Run; Worker leasing and execution are observed here and not
        triggered by the Office.
      </p>
      {error && <p className="error">{error}</p>}
      {notice && <p className="notice">{notice}</p>}

      <h3>All Runs ({runs.length})</h3>
      {runs.length === 0 ? (
        <p className="muted">
          No Runs yet. Authorize a Campaign and queue a Run first.
        </p>
      ) : (
        <table>
          <thead>
            <tr>
              <th>Run</th>
              <th>Campaign</th>
              <th>Status</th>
              <th>Attempts</th>
              <th>Created</th>
              <th>Control</th>
            </tr>
          </thead>
          <tbody>
            {runs.map((run) => {
              const runAttempts = attemptsByRun.get(run.id) ?? [];
              return (
                <tr key={run.id}>
                  <td>
                    <code>{shortId(run.id)}</code>
                  </td>
                  <td>
                    <code>{shortId(run.campaign_id)}</code>
                  </td>
                  <td>
                    <span className="pill">{run.status}</span>
                  </td>
                  <td>{runAttempts.length}</td>
                  <td className="muted">{formatTimestamp(run.created_at)}</td>
                  <td>
                    {run.status === "QUEUED" && runAttempts.length === 0 ? (
                      <button
                        type="button"
                        disabled={busyId === run.id}
                        onClick={() => void onQueueAttempt(run)}
                      >
                        {busyId === run.id ? "Queueing…" : "Queue Attempt"}
                      </button>
                    ) : (
                      <span className="muted">
                        {runAttempts.length > 0 ? "attempt queued" : "—"}
                      </span>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}

      <h3>All Attempts ({attempts.length})</h3>
      {attempts.length === 0 ? (
        <p className="muted">No Attempts queued.</p>
      ) : (
        <table>
          <thead>
            <tr>
              <th>Attempt</th>
              <th>Run</th>
              <th>Status</th>
              <th>Created</th>
            </tr>
          </thead>
          <tbody>
            {attempts.map((attempt) => (
              <tr key={attempt.id}>
                <td>
                  <code>{shortId(attempt.id)}</code>
                </td>
                <td>
                  <code>{shortId(attempt.run_id)}</code>
                </td>
                <td>
                  <span className="pill">{attempt.status}</span>
                </td>
                <td className="muted">{formatTimestamp(attempt.created_at)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
