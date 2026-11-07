import { useCallback, useEffect, useState } from "react";
import { CoreApiError, coreApi, type DelegationProjection } from "../coreApi";
import { formatTimestamp, shortId } from "../format";
import { useI18n } from "../i18n";
import { isDelegationExpired } from "../leadDelegations";

export function Delegations() {
  const { t } = useI18n();
  const [delegations, setDelegations] = useState<DelegationProjection[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [usage, setUsage] = useState<
    Record<string, { requests: number; sessions: number; attempts: number }>
  >({});
  const load = useCallback(async () => {
    const nextDelegations = await coreApi.listDelegations();
    setDelegations(nextDelegations);
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
                const expired = !historical && isDelegationExpired(delegation);
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
                        className={`pill ${
                          delegation.revokedAt || expired
                            ? "blocked"
                            : "accepted"
                        }`}
                      >
                        {historical
                          ? "HISTORICAL"
                          : delegation.revokedAt
                            ? "REVOKED"
                            : expired
                              ? "EXPIRED"
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
                      {!historical && !delegation.revokedAt && !expired && (
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
