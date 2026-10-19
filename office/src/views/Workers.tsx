import { useCallback, useEffect, useState } from "react";
import { ApiError, api, type Worker } from "../api";

// Workers view: Core-observed Worker liveness only — identity, status,
// last heartbeat and last assignment. Activity, providers and token usage
// are never rendered because the Core never records them.
export function Workers() {
  const [workers, setWorkers] = useState<Worker[]>([]);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async (signal?: AbortSignal) => {
    try {
      const list = await api.listWorkers();
      if (!signal?.aborted) {
        setWorkers(list);
      }
    } catch (e) {
      if (!signal?.aborted) {
        setError(e instanceof ApiError ? e.message : "Failed to load workers.");
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

  if (error) {
    return <p className="error">{error}</p>;
  }
  return (
    <div>
      <h2>Workers</h2>
      {workers.length === 0 ? (
        <p className="muted">
          No Workers registered. Start one with{" "}
          <code>factory worker start</code> — it appears here after its first
          heartbeat.
        </p>
      ) : (
        <table>
          <thead>
            <tr>
              <th>ID</th>
              <th>Status</th>
              <th>Version</th>
              <th>Last heartbeat</th>
              <th>Last Run</th>
            </tr>
          </thead>
          <tbody>
            {workers.map((w) => (
              <tr key={w.id}>
                <td>
                  <code>{w.id}</code>
                </td>
                <td>
                  <span className={`pill ${w.status}`}>{w.status}</span>
                </td>
                <td className="muted">{w.version || "—"}</td>
                <td className="muted">
                  {w.lastHeartbeat
                    ? new Date(w.lastHeartbeat).toLocaleString()
                    : "never"}
                </td>
                <td className="muted">
                  {w.lastRunId ? w.lastRunId.slice(0, 8) : "—"}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
