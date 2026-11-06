import { useEffect, useState } from "react";
import { CoreApiError, coreApi, type WorkerSummary } from "../coreApi";
import { useI18n } from "../i18n";

export function Workers() {
  const { t, formatDate } = useI18n();
  const [workers, setWorkers] = useState<WorkerSummary[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let mounted = true;
    async function load() {
      try {
        const next = await coreApi.listWorkers();
        if (mounted) {
          setWorkers(next);
          setError(null);
        }
      } catch (cause) {
        if (mounted) {
          setError(
            cause instanceof CoreApiError
              ? cause.message
              : t("workers.error.load"),
          );
        }
      }
    }
    void load();
    const timer = setInterval(() => void load(), 3000);
    return () => {
      mounted = false;
      clearInterval(timer);
    };
  }, [t]);

  return (
    <div className="workers-workspace">
      <h2>{t("workers.title")}</h2>
      <p className="muted">{t("workers.description")}</p>
      {error && <p className="error">{error}</p>}
      <h3>{t("workers.section.registered", { count: workers.length })}</h3>
      {workers.length === 0 ? (
        <p className="muted">{t("workers.list.empty")}</p>
      ) : (
        <section
          className="table-responsive"
          aria-label={t("workers.table.label")}
        >
          <table>
            <thead>
              <tr>
                <th>{t("workers.table.worker")}</th>
                <th>{t("workers.table.credentialUpdated")}</th>
              </tr>
            </thead>
            <tbody>
              {workers.map((worker) => (
                <tr key={worker.worker_id}>
                  <td data-label={t("workers.table.worker")}>
                    <code>{worker.worker_id}</code>
                  </td>
                  <td
                    className="muted"
                    data-label={t("workers.table.credentialUpdated")}
                  >
                    {formatDate(worker.updated_at)}
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
