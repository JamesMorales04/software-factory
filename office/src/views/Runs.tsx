import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { type Attempt, CoreApiError, coreApi, type Run } from "../coreApi";
import { shortId } from "../format";
import { useI18n } from "../i18n";

export function Runs() {
  const { t, formatDate } = useI18n();
  const [runs, setRuns] = useState<Run[]>([]);
  const [attempts, setAttempts] = useState<Attempt[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);

  useEffect(() => {
    let mounted = true;
    async function load() {
      try {
        const [nextRuns, nextAttempts] = await Promise.all([
          coreApi.listRuns(),
          coreApi.listAttempts(),
        ]);
        if (mounted) {
          setRuns(nextRuns);
          setAttempts(nextAttempts);
        }
      } catch (cause) {
        if (mounted) {
          setError(
            cause instanceof CoreApiError
              ? cause.message
              : t("runs.error.load"),
          );
        }
      }
    }
    void load();
    const timer = setInterval(() => void load(), 2000);
    return () => {
      mounted = false;
      clearInterval(timer);
    };
  }, [t]);

  async function onQueueAttempt(run: Run) {
    setBusyId(run.id);
    setError(null);
    setNotice(null);
    try {
      const attempt = await coreApi.createRunAttempt(run.id);
      setNotice(
        t("runs.notice.queued", {
          attempt: shortId(attempt.id),
          run: shortId(run.id),
        }),
      );
      const [nextRuns, nextAttempts] = await Promise.all([
        coreApi.listRuns(),
        coreApi.listAttempts(),
      ]);
      setRuns(nextRuns);
      setAttempts(nextAttempts);
    } catch (cause) {
      setError(
        cause instanceof CoreApiError ? cause.message : t("runs.error.queue"),
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
    <div className="runs-workspace">
      <h2>{t("runs.title")}</h2>
      <p className="muted">{t("runs.description")}</p>
      {error && <p className="error">{error}</p>}
      {notice && <p className="notice">{notice}</p>}

      <h3>{t("runs.section.allRuns", { count: runs.length })}</h3>
      {runs.length === 0 ? (
        <p className="muted">{t("runs.list.empty")}</p>
      ) : (
        <section
          className="table-responsive"
          aria-label={t("runs.table.label")}
        >
          <table>
            <thead>
              <tr>
                <th>{t("runs.table.run")}</th>
                <th>{t("runs.table.campaign")}</th>
                <th>{t("runs.table.status")}</th>
                <th>{t("runs.table.attempts")}</th>
                <th>{t("runs.table.created")}</th>
                <th>{t("runs.table.control")}</th>
              </tr>
            </thead>
            <tbody>
              {runs.map((run) => {
                const runAttempts = attemptsByRun.get(run.id) ?? [];
                return (
                  <tr key={run.id}>
                    <td data-label={t("runs.table.run")}>
                      <code>{shortId(run.id)}</code>
                    </td>
                    <td data-label={t("runs.table.campaign")}>
                      <Link to={`/campaigns/${run.campaign_id}`}>
                        <code>{shortId(run.campaign_id)}</code>
                      </Link>
                    </td>
                    <td data-label={t("runs.table.status")}>
                      <span className="pill">{run.status}</span>
                    </td>
                    <td data-label={t("runs.table.attempts")}>
                      {runAttempts.length}
                    </td>
                    <td className="muted" data-label={t("runs.table.created")}>
                      {formatDate(run.created_at)}
                    </td>
                    <td data-label={t("runs.table.control")}>
                      {run.status === "QUEUED" && runAttempts.length === 0 ? (
                        <button
                          type="button"
                          disabled={busyId === run.id}
                          onClick={() => void onQueueAttempt(run)}
                        >
                          {busyId === run.id
                            ? t("runs.queueing")
                            : t("runs.queue")}
                        </button>
                      ) : (
                        <span className="muted">
                          {runAttempts.length > 0
                            ? t("runs.attemptQueued")
                            : t("common.dash")}
                        </span>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </section>
      )}

      <h3>{t("runs.section.allAttempts", { count: attempts.length })}</h3>
      {attempts.length === 0 ? (
        <p className="muted">{t("runs.attempts.empty")}</p>
      ) : (
        <section
          className="table-responsive"
          aria-label={t("runs.table.label")}
        >
          <table>
            <thead>
              <tr>
                <th>{t("runs.table.attempt")}</th>
                <th>{t("runs.table.run")}</th>
                <th>{t("runs.table.status")}</th>
                <th>{t("runs.table.created")}</th>
              </tr>
            </thead>
            <tbody>
              {attempts.map((attempt) => (
                <tr key={attempt.id}>
                  <td data-label={t("runs.table.attempt")}>
                    <code>{shortId(attempt.id)}</code>
                  </td>
                  <td data-label={t("runs.table.run")}>
                    <code>{shortId(attempt.run_id)}</code>
                  </td>
                  <td data-label={t("runs.table.status")}>
                    <span className="pill">{attempt.status}</span>
                  </td>
                  <td className="muted" data-label={t("runs.table.created")}>
                    {formatDate(attempt.created_at)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}
    </div>
  );
}
