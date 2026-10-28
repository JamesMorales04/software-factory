import { useEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";
import {
  CoreApiError,
  coreApi,
  newIdempotencyKey,
  type Project,
  type ProjectConfig,
} from "../coreApi";
import { shortId } from "../format";
import { useI18n } from "../i18n";

export function Projects() {
  const { t, formatDate } = useI18n();
  const [projects, setProjects] = useState<Project[]>([]);
  const [repository, setRepository] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [selected, setSelected] = useState<Project | null>(null);
  const [config, setConfig] = useState<ProjectConfig | null>(null);
  const [configPaths, setConfigPaths] = useState("");
  const [configCapabilities, setConfigCapabilities] = useState<string[]>([]);
  const [qualityProfile, setQualityProfile] = useState("generic");
  const [active, setActive] = useState(true);
  const attemptKey = useRef(newIdempotencyKey());

  useEffect(() => {
    let mounted = true;
    async function load() {
      try {
        const next = await coreApi.listProjects();
        if (mounted) {
          setProjects(next);
        }
      } catch (cause) {
        if (mounted) {
          setError(
            cause instanceof CoreApiError
              ? cause.message
              : t("projects.error.load"),
          );
        }
      }
    }
    void load();
    const timer = setInterval(() => void load(), 4000);
    return () => {
      mounted = false;
      clearInterval(timer);
    };
  }, [t]);

  async function onRegister(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const project = await coreApi.registerProject({
        repository: repository.trim(),
        idempotency_key: attemptKey.current,
      });
      attemptKey.current = newIdempotencyKey();
      setNotice(
        t("projects.register.notice", {
          id: shortId(project.id),
          commit: shortId(project.base_commit, 12),
        }),
      );
      setRepository("");
      const next = await coreApi.listProjects();
      setProjects(next);
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : t("projects.error.register"),
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="projects-workspace">
      <h2>{t("projects.title")}</h2>
      <p className="muted">{t("projects.description")}</p>
      {error && <p className="error">{error}</p>}
      {notice && <p className="notice">{notice}</p>}

      <form className="create" onSubmit={onRegister}>
        <h3>{t("projects.register.title")}</h3>
        <label>
          {t("projects.register.label")}
          <input
            data-testid="project-repository"
            value={repository}
            onChange={(event) => {
              setRepository(event.target.value);
              attemptKey.current = newIdempotencyKey();
            }}
            placeholder="https://github.com/owner/repository.git"
            required
          />
        </label>
        <button type="submit" disabled={busy || repository.trim() === ""}>
          {busy ? t("projects.register.busy") : t("projects.register.submit")}
        </button>
      </form>

      <h3>{t("projects.list.title", { count: projects.length })}</h3>
      {projects.length === 0 ? (
        <p className="muted">{t("projects.list.empty")}</p>
      ) : (
        <section
          className="table-responsive"
          aria-label={t("projects.table.label")}
        >
          <table>
            <thead>
              <tr>
                <th>{t("projects.table.id")}</th>
                <th>{t("projects.table.repository")}</th>
                <th>{t("projects.table.baseCommit")}</th>
                <th>{t("projects.table.created")}</th>
                <th>{t("projects.table.actions")}</th>
              </tr>
            </thead>
            <tbody>
              {projects.map((project) => (
                <tr key={project.id}>
                  <td data-label={t("projects.table.id")}>
                    <code>{shortId(project.id)}</code>
                  </td>
                  <td data-label={t("projects.table.repository")}>
                    <code>{project.repository}</code>
                  </td>
                  <td data-label={t("projects.table.baseCommit")}>
                    <code title={project.base_commit}>
                      {shortId(project.base_commit, 12)}
                    </code>
                  </td>
                  <td
                    className="muted"
                    data-label={t("projects.table.created")}
                  >
                    {formatDate(project.created_at)}
                  </td>
                  <td
                    className="project-actions"
                    data-label={t("projects.table.actions")}
                  >
                    <Link
                      className="ui-button ui-button--secondary ui-button--sm"
                      to={`/projects/${encodeURIComponent(project.id)}/files`}
                    >
                      {t("projects.browseFiles")}
                    </Link>
                    <button
                      type="button"
                      onClick={() => {
                        setSelected(project);
                        setError(null);
                        void coreApi
                          .getProjectConfig(project.id)
                          .then((next) => {
                            setConfig(next);
                            setConfigPaths(next.allowedPaths.join(", "));
                            setConfigCapabilities(next.capabilities);
                            setQualityProfile(next.qualityProfile);
                            setActive(next.active);
                          })
                          .catch((cause) =>
                            setError(
                              cause instanceof CoreApiError
                                ? cause.message
                                : t("projects.error.configLoad"),
                            ),
                          );
                      }}
                    >
                      {t("projects.configure")}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}

      {selected && config && (
        <form
          className="create"
          onSubmit={(event) => {
            event.preventDefault();
            setBusy(true);
            setError(null);
            void coreApi
              .updateProjectConfig(selected.id, {
                expectedRevision: config.revision,
                allowedPaths: configPaths
                  .split(",")
                  .map((path) => path.trim())
                  .filter(Boolean),
                capabilities: configCapabilities,
                qualityProfile,
                active,
              })
              .then((next) => {
                setConfig(next);
                setNotice(
                  t("projects.config.notice", {
                    revision: next.revision,
                    hash: next.configHash.slice(0, 12),
                  }),
                );
              })
              .catch((cause) =>
                setError(
                  cause instanceof CoreApiError
                    ? cause.message
                    : t("projects.error.configSave"),
                ),
              )
              .finally(() => setBusy(false));
          }}
        >
          <h3>{t("projects.config.title", { id: shortId(selected.id) })}</h3>
          <p className="muted">
            {t("projects.config.revision", { revision: config.revision })} ·{" "}
            {t("projects.config.base")}{" "}
            <code>{shortId(config.baseCommit, 12)}</code> ·{" "}
            {t("projects.config.hash")}{" "}
            <code>{shortId(config.configHash, 12)}</code>
          </p>
          <label>
            {t("projects.config.allowedPaths")}
            <input
              value={configPaths}
              onChange={(event) => setConfigPaths(event.target.value)}
              placeholder={t("projects.config.allowedPathsPlaceholder")}
            />
          </label>
          <fieldset>
            <legend>{t("projects.config.capabilities")}</legend>
            {[
              "repository-edit",
              "generic-toolchain",
              "node-toolchain",
              "browser",
            ].map((capability) => (
              <label className="inline" key={capability}>
                <input
                  type="checkbox"
                  checked={configCapabilities.includes(capability)}
                  onChange={(event) =>
                    setConfigCapabilities((current) =>
                      event.target.checked
                        ? [...current, capability]
                        : current.filter((item) => item !== capability),
                    )
                  }
                />
                {capability}
              </label>
            ))}
          </fieldset>
          <div className="grid-2">
            <label>
              {t("projects.config.qualityProfile")}
              <select
                value={qualityProfile}
                onChange={(event) => setQualityProfile(event.target.value)}
              >
                <option value="generic">generic</option>
                <option value="node-basic">node-basic</option>
                <option value="node-browser">node-browser</option>
              </select>
            </label>
            <label className="inline">
              <input
                type="checkbox"
                checked={active}
                onChange={(event) => setActive(event.target.checked)}
              />
              {t("projects.config.active")}
            </label>
          </div>
          <button type="submit" disabled={busy}>
            {busy ? t("projects.config.saving") : t("projects.config.save")}
          </button>
        </form>
      )}
    </div>
  );
}
