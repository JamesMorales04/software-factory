import { useCallback, useEffect, useRef, useState } from "react";
import {
  CoreApiError,
  coreApi,
  newIdempotencyKey,
  type Project,
  type StandingDelegation,
} from "../coreApi";
import { formatTimestamp, shortId } from "../format";

function split(value: string): string[] {
  return value
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
}

export function Delegations() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [delegations, setDelegations] = useState<StandingDelegation[]>([]);
  const [projectId, setProjectId] = useState("");
  const [newDelegationId, setNewDelegationId] = useState("");
  const [paths, setPaths] = useState("src");
  const [provider, setProvider] = useState("opencode-go");
  const [model, setModel] = useState("deepseek-v4.1-flash");
  const [mode, setMode] = useState("build");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [usage, setUsage] = useState<
    Record<string, { requests: number; sessions: number; attempts: number }>
  >({});
  const key = useRef(newIdempotencyKey());

  const load = useCallback(async () => {
    const [nextProjects, nextDelegations] = await Promise.all([
      coreApi.listProjects(),
      coreApi.listDelegations(),
    ]);
    setProjects(nextProjects);
    setDelegations(nextDelegations);
    setProjectId((current) => current || nextProjects[0]?.id || "");
    const details = await Promise.all(
      nextDelegations.map(async (delegation) => {
        try {
          const detail = await coreApi.getDelegation(delegation.id);
          return [
            delegation.id,
            {
              requests: detail.usage.requests,
              sessions: detail.usage.sessions,
              attempts: detail.usage.attempts,
            },
          ] as const;
        } catch {
          return null;
        }
      }),
    );
    setUsage(Object.fromEntries(details.filter((item) => item !== null)));
  }, []);

  useEffect(() => {
    let active = true;
    void load().catch((cause) => {
      if (active) {
        setError(
          cause instanceof CoreApiError
            ? cause.message
            : "Failed to load delegations.",
        );
      }
    });
    const timer = setInterval(() => {
      void load().catch(() => undefined);
    }, 5000);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [load]);

  async function onCreate(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      if (!projectId) throw new Error("Select a Project.");
      const id =
        newDelegationId.trim() ||
        `office-${new Date().toISOString().replace(/[:.]/g, "-")}`;
      const allowedPaths = split(paths);
      if (allowedPaths.length === 0)
        throw new Error("At least one path is required.");
      const expiresAt = new Date(
        Date.now() + 24 * 60 * 60 * 1000,
      ).toISOString();
      const delegation = await coreApi.createDelegation({
        idempotency_key: key.current,
        delegation: {
          id,
          principalId: "owner",
          auditId: "office-delegation",
          projectId,
          revision: 1,
          contentHash: "",
          objectiveClasses: ["feature"],
          allowedPaths,
          capabilities: ["repository-edit", "generic-toolchain"],
          riskClasses: ["R0", "R1"],
          autonomousStart: false,
          autonomousRiskClasses: [],
          route: { provider, model: model.trim(), mode },
          maxAttempts: 2,
          maxTimeoutSecs: 300,
          maxRepairCycles: 0,
          maxWallClockSecs: 1800,
          maxParallel: 1,
          maxSessions: 16,
          maxRequests: 4,
          maxCumulativeSessions: 32,
          maxCumulativeAttempts: 32,
          expiresAt,
        },
      });
      key.current = newIdempotencyKey();
      setNotice(
        `Delegation ${shortId(delegation.id)} created with revision ${delegation.revision}.`,
      );
      setNewDelegationId("");
      await load();
    } catch (cause) {
      setError(
        cause instanceof Error ? cause.message : "Delegation creation failed.",
      );
    } finally {
      setBusy(false);
    }
  }

  async function revoke(id: string) {
    if (!window.confirm(`Revoke delegation ${id}?`)) return;
    setBusy(true);
    setError(null);
    try {
      await coreApi.revokeDelegation(id);
      setNotice(`Delegation ${shortId(id)} revoked.`);
      await load();
    } catch (cause) {
      setError(
        cause instanceof CoreApiError ? cause.message : "Revocation failed.",
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <div>
      <h2>Delegations</h2>
      <p className="muted">
        A delegation is a scoped, revocable grant. It does not start work by
        itself and autonomous execution defaults to disabled.
      </p>
      {error && <p className="error">{error}</p>}
      {notice && <p className="notice">{notice}</p>}
      <form className="create" onSubmit={(event) => void onCreate(event)}>
        <h3>New delegation</h3>
        <div className="grid-2">
          <label>
            Project
            <select
              value={projectId}
              onChange={(event) => setProjectId(event.target.value)}
            >
              {projects.map((project) => (
                <option key={project.id} value={project.id}>
                  {project.repository} ({shortId(project.id)})
                </option>
              ))}
            </select>
          </label>
          <label>
            Delegation ID (blank for generated)
            <input
              value={newDelegationId}
              onChange={(event) => setNewDelegationId(event.target.value)}
              placeholder="generated when blank"
            />
          </label>
          <label>
            Provider
            <select
              value={provider}
              onChange={(event) => setProvider(event.target.value)}
            >
              <option value="opencode-go">opencode-go</option>
              <option value="opencode">opencode</option>
              <option value="codex">codex</option>
            </select>
          </label>
          <label>
            Model
            <input
              value={model}
              onChange={(event) => setModel(event.target.value)}
              required
            />
          </label>
          <label>
            Mode
            <input
              value={mode}
              onChange={(event) => setMode(event.target.value)}
              required
            />
          </label>
          <label>
            Allowed paths (comma separated)
            <input
              value={paths}
              onChange={(event) => setPaths(event.target.value)}
              required
            />
          </label>
        </div>
        <p className="muted">
          Limits: 2 attempts, 16 sessions, 4 requests, 32 cumulative attempts,
          300s per attempt, 24h expiry. R0/R1 only; R2/R3 require protected
          approval.
        </p>
        <button type="submit" disabled={busy || !projectId}>
          {busy ? "Creating…" : "Create delegation"}
        </button>
      </form>

      <h3>Delegations ({delegations.length})</h3>
      {delegations.length === 0 ? (
        <p className="muted">No delegations.</p>
      ) : (
        <table>
          <thead>
            <tr>
              <th>ID</th>
              <th>Project</th>
              <th>Route</th>
              <th>Paths</th>
              <th>State</th>
              <th>Usage</th>
              <th>Expires</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {delegations.map((delegation) => (
              <tr key={delegation.id}>
                <td>
                  <code>{delegation.id}</code>
                </td>
                <td>
                  <code>{shortId(delegation.projectId)}</code>
                </td>
                <td>
                  <code>
                    {delegation.route.provider}/{delegation.route.model}
                  </code>
                  <div className="muted">{delegation.route.mode}</div>
                </td>
                <td>{delegation.allowedPaths.join(", ")}</td>
                <td>
                  <span
                    className={`pill ${delegation.revokedAt ? "blocked" : "accepted"}`}
                  >
                    {delegation.revokedAt ? "REVOKED" : "ACTIVE"}
                  </span>
                  {delegation.autonomousStart && (
                    <span className="pill blocked">autonomous</span>
                  )}
                </td>
                <td className="muted">
                  {usage[delegation.id]
                    ? `${usage[delegation.id].requests} req · ${usage[delegation.id].sessions} sess · ${usage[delegation.id].attempts} att`
                    : "—"}
                </td>
                <td className="muted">
                  {formatTimestamp(delegation.expiresAt)}
                </td>
                <td>
                  {!delegation.revokedAt && (
                    <button
                      type="button"
                      className="danger"
                      disabled={busy}
                      onClick={() => void revoke(delegation.id)}
                    >
                      Revoke
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
