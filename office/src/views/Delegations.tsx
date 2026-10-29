import { useCallback, useEffect, useRef, useState } from "react";
import {
  CoreApiError,
  coreApi,
  type DelegationProjection,
  newIdempotencyKey,
  type Project,
} from "../coreApi";
import { formatTimestamp, shortId } from "../format";
import { useI18n } from "../i18n";

function split(value: string): string[] {
  return value
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
}

export function Delegations() {
  const { t } = useI18n();
  const [projects, setProjects] = useState<Project[]>([]);
  const [delegations, setDelegations] = useState<DelegationProjection[]>([]);
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
      nextDelegations.map(async (projection) => {
        const delegation = projection.delegation;
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
            : t("delegations.error.load"),
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
  }, [load, t]);

  async function onCreate(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      if (!projectId) throw new Error(t("delegations.validation.project"));
      const id =
        newDelegationId.trim() ||
        `office-${new Date().toISOString().replace(/[:.]/g, "-")}`;
      const allowedPaths = split(paths);
      if (allowedPaths.length === 0)
        throw new Error(t("delegations.validation.paths"));
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
        t("delegations.notice.created", {
          id: shortId(delegation.id),
          revision: delegation.revision,
        }),
      );
      setNewDelegationId("");
      await load();
    } catch (cause) {
      setError(
        cause instanceof Error ? cause.message : t("delegations.error.create"),
      );
    } finally {
      setBusy(false);
    }
  }

  async function revoke(id: string) {
    if (!window.confirm(t("delegations.confirm.revoke", { id }))) return;
    setBusy(true);
    setError(null);
    try {
      await coreApi.revokeDelegation(id);
      setNotice(t("delegations.notice.revoked", { id: shortId(id) }));
      await load();
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : t("delegations.error.revoke"),
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="delegations-workspace">
      <h2>{t("delegations.title")}</h2>
      <p className="muted">{t("delegations.description")}</p>
      {error && <p className="error">{error}</p>}
      {notice && <p className="notice">{notice}</p>}
      <form className="create" onSubmit={(event) => void onCreate(event)}>
        <h3>{t("delegations.form.title")}</h3>
        <div className="grid-2">
          <label>
            {t("delegations.form.project")}
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
            {t("delegations.form.id")}
            <input
              value={newDelegationId}
              onChange={(event) => setNewDelegationId(event.target.value)}
              placeholder={t("delegations.form.idPlaceholder")}
            />
          </label>
          <label>
            {t("delegations.form.provider")}
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
            {t("delegations.form.model")}
            <input
              value={model}
              onChange={(event) => setModel(event.target.value)}
              required
            />
          </label>
          <label>
            {t("delegations.form.mode")}
            <input
              value={mode}
              onChange={(event) => setMode(event.target.value)}
              required
            />
          </label>
          <label>
            {t("delegations.form.paths")}
            <input
              value={paths}
              onChange={(event) => setPaths(event.target.value)}
              required
            />
          </label>
        </div>
        <p className="muted">{t("delegations.form.limits")}</p>
        <button type="submit" disabled={busy || !projectId}>
          {busy ? t("delegations.form.creating") : t("delegations.form.create")}
        </button>
      </form>

      <h3>{t("delegations.list.title", { count: delegations.length })}</h3>
      {delegations.length === 0 ? (
        <p className="muted">{t("delegations.list.empty")}</p>
      ) : (
        <div className="table-responsive">
          <table>
            <thead>
              <tr>
                <th>{t("delegations.table.id")}</th>
                <th>{t("delegations.table.project")}</th>
                <th>{t("delegations.table.route")}</th>
                <th>{t("delegations.table.paths")}</th>
                <th>{t("delegations.table.state")}</th>
                <th>{t("delegations.table.usage")}</th>
                <th>{t("delegations.table.expires")}</th>
                <th>
                  <span className="ui-visually-hidden">
                    {t("delegations.table.actions")}
                  </span>
                </th>
              </tr>
            </thead>
            <tbody>
              {delegations.map((projection) => {
                const delegation = projection.delegation;
                const historical = projection.schemaVersion === 1;
                return (
                  <tr key={delegation.id}>
                    <td data-label={t("delegations.table.id")}>
                      <code>{delegation.id}</code>
                    </td>
                    <td data-label={t("delegations.table.project")}>
                      <code>{shortId(delegation.projectId)}</code>
                    </td>
                    <td data-label={t("delegations.table.route")}>
                      <code>
                        {delegation.route.provider}/{delegation.route.model}
                      </code>
                      <div className="muted">{delegation.route.mode}</div>
                    </td>
                    <td data-label={t("delegations.table.paths")}>
                      {delegation.allowedPaths.join(", ")}
                    </td>
                    <td data-label={t("delegations.table.state")}>
                      <span
                        className={`pill ${delegation.revokedAt ? "blocked" : "accepted"}`}
                      >
                        {historical
                          ? "HISTORICAL"
                          : delegation.revokedAt
                            ? "REVOKED"
                            : "ACTIVE"}
                      </span>
                      {!historical && delegation.autonomousStart && (
                        <span className="pill blocked">
                          {t("delegations.table.autonomous")}
                        </span>
                      )}
                    </td>
                    <td
                      className="muted"
                      data-label={t("delegations.table.usage")}
                    >
                      {usage[delegation.id]
                        ? t("delegations.table.usageValue", {
                            requests: usage[delegation.id].requests,
                            sessions: usage[delegation.id].sessions,
                            attempts: usage[delegation.id].attempts,
                          })
                        : "—"}
                    </td>
                    <td
                      className="muted"
                      data-label={t("delegations.table.expires")}
                    >
                      {formatTimestamp(delegation.expiresAt)}
                    </td>
                    <td data-label={t("delegations.table.actions")}>
                      {!historical && !delegation.revokedAt && (
                        <button
                          type="button"
                          className="danger"
                          disabled={busy}
                          onClick={() => void revoke(delegation.id)}
                        >
                          {t("delegations.table.revoke")}
                        </button>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
