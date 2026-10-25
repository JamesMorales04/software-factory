import { useEffect, useState } from "react";
import { CoreApiError, coreApi, type WorkerSummary } from "../coreApi";
import { formatTimestamp } from "../format";

export function Workers() {
  const [workers, setWorkers] = useState<WorkerSummary[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    async function load() {
      try {
        const next = await coreApi.listWorkers();
        if (active) {
          setWorkers(next);
          setError(null);
        }
      } catch (cause) {
        if (active) {
          setError(
            cause instanceof CoreApiError
              ? cause.message
              : "Failed to load Workers.",
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

  return (
    <div>
      <h2>Workers</h2>
      <p className="muted">
        Registered Worker credentials only. This view does not fabricate
        liveness, heartbeat, or execution state.
      </p>
      {error && <p className="error">{error}</p>}
      <h3>Registered Workers ({workers.length})</h3>
      {workers.length === 0 ? (
        <p className="muted">No Workers registered.</p>
      ) : (
        <table>
          <thead>
            <tr>
              <th>Worker</th>
              <th>Credential updated</th>
            </tr>
          </thead>
          <tbody>
            {workers.map((worker) => (
              <tr key={worker.worker_id}>
                <td>
                  <code>{worker.worker_id}</code>
                </td>
                <td className="muted">{formatTimestamp(worker.updated_at)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
