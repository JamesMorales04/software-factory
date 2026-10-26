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
import { formatTimestamp, shortId } from "../format";

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
      description: "candidate content is present",
      command: ["grep", "-q", "provider candidate", "src/provider.txt"],
      commandText: 'grep -q "provider candidate" src/provider.txt',
      expectExit: 0,
      timeoutSecs: 30,
    },
    {
      draftId: crypto.randomUUID(),
      id: "negative",
      kind: "negative",
      description: "absent content is rejected",
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
              : "Failed to load Actions.",
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
  }, []);

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
        throw new Error("Every check needs a nonempty command.");
      }
      if (normalized.length === 0) {
        throw new Error(
          "At least one frozen check is required for independent verification.",
        );
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
        `Created Action ${shortId(action.id)} with spec ${shortId(action.spec_sha256, 12)}.`,
      );
      setObjective("");
      const next = await coreApi.listActions();
      setActions(next);
    } catch (cause) {
      setError(
        cause instanceof Error ? cause.message : "Action creation failed.",
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <div>
      <h2>Actions</h2>
      <p className="muted">
        An Action freezes the objective, provider route, allowed paths and the
        exact checks later Runs and independent verification bind to. The
        provider route must be configured in Settings for live execution.
      </p>
      {error && <p className="error">{error}</p>}
      {notice && <p className="notice">{notice}</p>}

      {projects.length === 0 ? (
        <p className="notice">
          Register a Project before creating an Action.{" "}
          <Link to="/projects">Open Projects</Link>
        </p>
      ) : (
        <form className="create" onSubmit={onCreate}>
          <h3>New Action</h3>
          <label>
            Project
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
            Objective
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
              placeholder="What should the Factory do?"
            />
          </label>
          <div className="grid-2">
            <label>
              Provider
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
              Model
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
            Allowed repository paths (comma separated)
            <input
              value={allowedPaths}
              onChange={(event) => {
                setAllowedPaths(event.target.value);
                resetKey();
              }}
              placeholder="src, tests"
            />
          </label>

          <h4>Frozen checks</h4>
          <p className="muted">
            Commands run with a cleared environment in a network-disabled child
            container. Quote arguments that contain spaces. Verification
            requires at least one executable check.
          </p>
          {checks.map((check, index) => (
            <fieldset key={check.draftId}>
              <legend>Check {index + 1}</legend>
              <div className="grid-2">
                <label>
                  ID
                  <input
                    value={check.id}
                    onChange={(event) =>
                      updateCheck(index, { id: event.target.value })
                    }
                    required
                  />
                </label>
                <label>
                  Kind
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
                Description
                <input
                  value={check.description}
                  onChange={(event) =>
                    updateCheck(index, { description: event.target.value })
                  }
                />
              </label>
              <label>
                Command
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
                  Expected exit code
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
                  Timeout (seconds)
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
                Preview:{" "}
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
                Remove check
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
            Add check
          </button>
          <button type="submit" disabled={busy || objective.trim() === ""}>
            {busy ? "Creating…" : "Create Action"}
          </button>
        </form>
      )}

      <h3>All Actions ({actions.length})</h3>
      {actions.length === 0 ? (
        <p className="muted">No Actions yet.</p>
      ) : (
        <table>
          <thead>
            <tr>
              <th>ID</th>
              <th>Project</th>
              <th>Objective</th>
              <th>Route</th>
              <th>Risk</th>
              <th>Status</th>
              <th>Checks</th>
              <th>Spec</th>
              <th>Created</th>
            </tr>
          </thead>
          <tbody>
            {actions.map((action) => (
              <tr key={action.id}>
                <td>
                  <code>{shortId(action.id)}</code>
                </td>
                <td>
                  <code>{shortId(action.project_id)}</code>
                </td>
                <td>
                  {action.objective}
                  <div className="muted">
                    paths {action.spec.allowed_paths.join(", ") || "—"}
                  </div>
                </td>
                <td>
                  <code>
                    {action.provider}/{action.model}
                  </code>
                </td>
                <td>{action.risk}</td>
                <td>
                  <span className={`pill ${action.status.toLowerCase()}`}>
                    {action.status}
                  </span>
                </td>
                <td>{action.spec.checks.length}</td>
                <td>
                  <code title={action.spec_sha256}>
                    {shortId(action.spec_sha256, 12)}
                  </code>
                </td>
                <td className="muted">{formatTimestamp(action.created_at)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
