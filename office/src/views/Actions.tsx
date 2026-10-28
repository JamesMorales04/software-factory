import { useEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";
import {
  type Action,
  type CheckKind,
  CoreApiError,
  coreApi,
  newIdempotencyKey,
  type Project,
  type ProviderView,
  type SpecCheck,
} from "../coreApi";
import { shortId } from "../format";
import { useI18n } from "../i18n";

const CHECK_KINDS: CheckKind[] = [
  "format",
  "lint",
  "build",
  "test",
  "functional",
  "negative",
  "browser",
];

function splitCommand(value: string): string[] {
  const output: string[] = [];
  let current = "";
  let quote: '"' | "'" | null = null;
  let escaped = false;
  for (const character of value) {
    if (escaped) {
      current += character;
      escaped = false;
      continue;
    }
    if (character === "\\") {
      escaped = true;
      continue;
    }
    if (quote) {
      if (character === quote) quote = null;
      else current += character;
      continue;
    }
    if (character === '"' || character === "'") {
      quote = character;
      continue;
    }
    if (character === " ") {
      if (current !== "") {
        output.push(current);
        current = "";
      }
      continue;
    }
    current += character;
  }
  if (current !== "") output.push(current);
  return output;
}

type DraftCheck = SpecCheck & { commandText: string; draftId: string };

function newCheck(): DraftCheck {
  return {
    draftId: crypto.randomUUID(),
    id: `check-${Date.now()}`,
    kind: "functional",
    description: "",
    command: ["true"],
    commandText: "true",
    expectExit: 0,
    timeoutSecs: 30,
  };
}

export function Actions() {
  const { t, formatDate, formatNumber } = useI18n();
  const [actions, setActions] = useState<Action[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [providers, setProviders] = useState<ProviderView[]>([]);
  const [projectId, setProjectId] = useState("");
  const [objective, setObjective] = useState("");
  const [provider, setProvider] = useState("opencode-go");
  const [model, setModel] = useState("deepseek-v4.1-flash");
  const [allowedPaths, setAllowedPaths] = useState("src");
  const [checks, setChecks] = useState<DraftCheck[]>([
    {
      draftId: crypto.randomUUID(),
      id: "functional",
      kind: "functional",
      description: t("actions.defaultCheck.functional"),
      command: ["grep", "-q", "provider candidate", "src/provider.txt"],
      commandText: 'grep -q "provider candidate" src/provider.txt',
      expectExit: 0,
      timeoutSecs: 30,
    },
    {
      draftId: crypto.randomUUID(),
      id: "negative",
      kind: "negative",
      description: t("actions.defaultCheck.negative"),
      command: ["grep", "-q", "absent", "src/provider.txt"],
      commandText: "grep -q absent src/provider.txt",
      expectExit: 1,
      timeoutSecs: 30,
    },
  ]);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const attemptKey = useRef(newIdempotencyKey());

  useEffect(() => {
    let active = true;
    async function load() {
      try {
        const [nextActions, nextProjects, providerResponse] = await Promise.all(
          [
            coreApi.listActions(),
            coreApi.listProjects(),
            coreApi.listProviders(),
          ],
        );
        if (!active) return;
        setActions(nextActions);
        setProjects(nextProjects);
        setProviders(providerResponse.providers);
        setProjectId((current) => current || nextProjects[0]?.id || "");
        const configured = providerResponse.providers.find(
          (item) => item.configured && item.enabled,
        );
        if (configured) {
          setProvider(configured.provider);
          setModel(configured.model);
        }
      } catch (cause) {
        if (active) {
          setError(
            cause instanceof CoreApiError
              ? cause.message
              : t("actions.error.load"),
          );
        }
      }
    }
    void load();
    const timer = setInterval(() => void load(), 4000);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [t]);

  function resetKey() {
    attemptKey.current = newIdempotencyKey();
  }

  function updateCheck(index: number, patch: Partial<DraftCheck>) {
    setChecks((current) =>
      current.map((check, position) =>
        position === index ? { ...check, ...patch } : check,
      ),
    );
    resetKey();
  }

  async function onCreate(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const normalized = checks.map((check) => ({
        id: check.id,
        kind: check.kind,
        description: check.description,
        command: splitCommand(check.commandText),
        expectExit: check.expectExit,
        timeoutSecs: check.timeoutSecs,
      }));
      if (normalized.some((check) => check.command.length === 0)) {
        throw new Error(t("actions.error.emptyCommand"));
      }
      if (normalized.length === 0) {
        throw new Error(t("actions.error.checkRequired"));
      }
      const action = await coreApi.createAction({
        project_id: projectId,
        objective: objective.trim(),
        provider,
        model: model.trim(),
        idempotency_key: attemptKey.current,
        allowed_paths: allowedPaths
          .split(",")
          .map((path) => path.trim())
          .filter(Boolean),
        checks: normalized,
      });
      attemptKey.current = newIdempotencyKey();
      setNotice(
        t("actions.notice.created", {
          id: shortId(action.id),
          spec: shortId(action.spec_sha256, 12),
        }),
      );
      setObjective("");
      const next = await coreApi.listActions();
      setActions(next);
    } catch (cause) {
      setError(
        cause instanceof Error ? cause.message : t("actions.error.create"),
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="actions-workspace">
      <h2>{t("actions.title")}</h2>
      <p className="muted">{t("actions.description")}</p>
      {error && <p className="error">{error}</p>}
      {notice && <p className="notice">{notice}</p>}

      {projects.length === 0 ? (
        <p className="notice">
          {t("actions.needProject")}{" "}
          <Link to="/projects">{t("actions.openProjects")}</Link>
        </p>
      ) : (
        <form className="create" onSubmit={onCreate}>
          <h3>{t("actions.new.title")}</h3>
          <label>
            {t("actions.project")}
            <select
              data-testid="action-project"
              value={projectId}
              onChange={(event) => {
                setProjectId(event.target.value);
                resetKey();
              }}
              required
            >
              {projects.map((project) => (
                <option key={project.id} value={project.id}>
                  {project.repository} ({shortId(project.id)})
                </option>
              ))}
            </select>
          </label>
          <label>
            {t("actions.objective")}
            <textarea
              data-testid="action-objective"
              value={objective}
              onChange={(event) => {
                setObjective(event.target.value);
                resetKey();
              }}
              maxLength={2000}
              rows={3}
              required
              placeholder={t("actions.objective.placeholder")}
            />
          </label>
          <div className="grid-2">
            <label>
              {t("actions.provider")}
              <select
                value={provider}
                onChange={(event) => {
                  const selected = providers.find(
                    (item) => item.provider === event.target.value,
                  );
                  setProvider(event.target.value);
                  if (selected) setModel(selected.defaultModel);
                  resetKey();
                }}
              >
                {(providers.length > 0
                  ? providers
                  : [
                      {
                        provider: "opencode-go",
                        defaultModel: "deepseek-v4.1-flash",
                      },
                      {
                        provider: "opencode",
                        defaultModel: "deepseek-v4.1-flash",
                      },
                      { provider: "codex", defaultModel: "gpt-6-luna" },
                    ]
                ).map((item) => (
                  <option key={item.provider} value={item.provider}>
                    {item.provider}
                  </option>
                ))}
              </select>
            </label>
            <label>
              {t("actions.model")}
              <input
                value={model}
                onChange={(event) => {
                  setModel(event.target.value);
                  resetKey();
                }}
                maxLength={128}
                required
                placeholder="deepseek-v4.1-flash"
              />
            </label>
          </div>
          <label>
            {t("actions.paths")}
            <input
              value={allowedPaths}
              onChange={(event) => {
                setAllowedPaths(event.target.value);
                resetKey();
              }}
              placeholder={t("actions.paths.placeholder")}
            />
          </label>

          <h4>{t("actions.checks.title")}</h4>
          <p className="muted">{t("actions.checks.help")}</p>
          {checks.map((check, index) => (
            <fieldset key={check.draftId}>
              <legend>{t("actions.check.legend", { index: index + 1 })}</legend>
              <div className="grid-2">
                <label>
                  {t("actions.check.id")}
                  <input
                    value={check.id}
                    onChange={(event) =>
                      updateCheck(index, { id: event.target.value })
                    }
                    required
                  />
                </label>
                <label>
                  {t("actions.check.kind")}
                  <select
                    value={check.kind}
                    onChange={(event) =>
                      updateCheck(index, {
                        kind: event.target.value as CheckKind,
                      })
                    }
                  >
                    {CHECK_KINDS.map((kind) => (
                      <option key={kind} value={kind}>
                        {kind}
                      </option>
                    ))}
                  </select>
                </label>
              </div>
              <label>
                {t("actions.check.description")}
                <input
                  value={check.description}
                  onChange={(event) =>
                    updateCheck(index, { description: event.target.value })
                  }
                />
              </label>
              <label>
                {t("actions.check.command")}
                <input
                  value={check.commandText}
                  onChange={(event) =>
                    updateCheck(index, {
                      commandText: event.target.value,
                    })
                  }
                  required
                />
              </label>
              <div className="grid-2">
                <label>
                  {t("actions.check.expectExit")}
                  <input
                    type="number"
                    value={check.expectExit}
                    onChange={(event) =>
                      updateCheck(index, {
                        expectExit: Number(event.target.value),
                      })
                    }
                  />
                </label>
                <label>
                  {t("actions.check.timeout")}
                  <input
                    type="number"
                    min={1}
                    max={1800}
                    value={check.timeoutSecs}
                    onChange={(event) =>
                      updateCheck(index, {
                        timeoutSecs: Number(event.target.value),
                      })
                    }
                  />
                </label>
              </div>
              <p className="muted">
                {t("actions.check.preview")}{" "}
                <code>{JSON.stringify(splitCommand(check.commandText))}</code>
              </p>
              <button
                type="button"
                className="danger"
                onClick={() => {
                  setChecks((current) =>
                    current.filter((_, position) => position !== index),
                  );
                  resetKey();
                }}
              >
                {t("actions.removeCheck")}
              </button>
            </fieldset>
          ))}
          <button
            type="button"
            onClick={() => {
              setChecks((current) => [...current, newCheck()]);
              resetKey();
            }}
          >
            {t("actions.addCheck")}
          </button>
          <button type="submit" disabled={busy || objective.trim() === ""}>
            {busy ? t("actions.creating") : t("actions.create")}
          </button>
        </form>
      )}

      <h3>{t("actions.all", { count: formatNumber(actions.length) })}</h3>
      {actions.length === 0 ? (
        <p className="muted">{t("actions.empty")}</p>
      ) : (
        <div className="table-responsive">
          <table aria-label={t("actions.table.label")}>
            <thead>
              <tr>
                <th>{t("actions.table.id")}</th>
                <th>{t("actions.table.project")}</th>
                <th>{t("actions.table.objective")}</th>
                <th>{t("actions.table.route")}</th>
                <th>{t("actions.table.risk")}</th>
                <th>{t("actions.table.status")}</th>
                <th>{t("actions.table.checks")}</th>
                <th>{t("actions.table.spec")}</th>
                <th>{t("actions.table.created")}</th>
              </tr>
            </thead>
            <tbody>
              {actions.map((action) => (
                <tr key={action.id}>
                  <td data-label={t("actions.table.id")}>
                    <Link to={`/actions/${action.id}`}>
                      <code>{shortId(action.id)}</code>
                    </Link>
                  </td>
                  <td data-label={t("actions.table.project")}>
                    <code>{shortId(action.project_id)}</code>
                  </td>
                  <td data-label={t("actions.table.objective")}>
                    {action.objective}
                    <div className="muted">
                      {t("actions.table.paths", {
                        paths: action.spec.allowed_paths.join(", ") || "—",
                      })}
                    </div>
                  </td>
                  <td data-label={t("actions.table.route")}>
                    <code>
                      {action.provider}/{action.model}
                    </code>
                  </td>
                  <td data-label={t("actions.table.risk")}>{action.risk}</td>
                  <td data-label={t("actions.table.status")}>
                    <span className={`pill ${action.status.toLowerCase()}`}>
                      {action.status}
                    </span>
                  </td>
                  <td data-label={t("actions.table.checks")}>
                    {formatNumber(action.spec.checks.length)}
                  </td>
                  <td data-label={t("actions.table.spec")}>
                    <code title={action.spec_sha256}>
                      {shortId(action.spec_sha256, 12)}
                    </code>
                  </td>
                  <td className="muted" data-label={t("actions.table.created")}>
                    {formatDate(action.created_at)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
