import { useCallback, useEffect, useMemo, useState } from "react";
import {
  type Action,
  type ActionSpec,
  ApiError,
  api,
  type Capability,
  type Project,
  type SpecCheck,
} from "../api";

const PROFILES = ["node-basic", "node-browser", "generic"] as const;

const ROUTES = [
  {
    label: "OpenCode Go · DeepSeek V4.1 Flash · build",
    provider: "opencode",
    model: "opencode-go/deepseek-v4.1-flash",
    mode: "build",
  },
  {
    label: "OpenCode Go · Muse Spark 1.3 Contributor · build",
    provider: "opencode",
    model: "opencode-go/muse-spark-1.3-contributor",
    mode: "build",
  },
  {
    label: "Codex · GPT-6 Luna · full-access",
    provider: "codex",
    model: "gpt-6-luna",
    mode: "full-access",
  },
];

interface CheckDraft {
  uid: string;
  id: string;
  kind: string;
  description: string;
  command: string;
  expectExit: number;
  timeoutSecs: number;
}

function blankCheck(kind: string): CheckDraft {
  return {
    uid:
      typeof crypto !== "undefined" && "randomUUID" in crypto
        ? crypto.randomUUID()
        : `check-${Math.random().toString(36).slice(2)}`,
    id: "",
    kind,
    description: "",
    command: "",
    expectExit: 0,
    timeoutSecs: 120,
  };
}

function toCheck(draft: CheckDraft): SpecCheck {
  return {
    id: draft.id.trim(),
    kind: draft.kind,
    description: draft.description.trim(),
    command: draft.command
      .split("\n")
      .map((item) => item.trim())
      .filter(Boolean),
    expectExit: draft.expectExit,
    timeoutSecs: draft.timeoutSecs,
  };
}

function templateChecks(profile: string): {
  acceptance: CheckDraft[];
  negative: CheckDraft[];
} {
  const functional = blankCheck("functional");
  functional.id = "functional";
  functional.description = "Task-specific functional assertion";
  const negative = blankCheck("negative");
  negative.id = "negative";
  negative.description = "Task-specific negative/regression assertion";
  if (profile === "generic") {
    const build = blankCheck("build");
    build.id = "build";
    build.description = "Project build";
    build.command = "make\nbuild";
    return { acceptance: [build, functional], negative: [negative] };
  }
  const format = blankCheck("format");
  format.id = "format";
  format.description = "Project formatter/static syntax check";
  format.command = "npm\nrun\nformat";
  const test = blankCheck("test");
  test.id = "test";
  test.description = "Project unit tests";
  test.command = "npm\ntest";
  const checks = [format, test, functional];
  if (profile === "node-browser") {
    const browser = blankCheck("browser");
    browser.id = "browser";
    browser.description = "Protected browser acceptance oracle";
    browser.command = "node\ntests/acceptance/browser-check.mjs";
    browser.timeoutSecs = 180;
    checks.push(browser);
  }
  return { acceptance: checks, negative: [negative] };
}

export function ActionSpecForm({
  action,
  project,
  onFrozen,
}: {
  action: Action;
  project: Project;
  onFrozen: (spec: ActionSpec) => void;
}) {
  const [goal, setGoal] = useState("");
  const [baseRevision, setBaseRevision] = useState("");
  const [baseTips, setBaseTips] = useState<Record<string, string>>({});
  const [allowedPaths, setAllowedPaths] = useState("");
  const [profile, setProfile] = useState<string>(
    project.qualityProfile || "node-basic",
  );
  const [risk, setRisk] = useState("R0");
  const [routeIndex, setRouteIndex] = useState(0);
  const [maxAttempts, setMaxAttempts] = useState(1);
  const [timeoutSecs, setTimeoutSecs] = useState(600);
  const [acceptance, setAcceptance] = useState<CheckDraft[]>([
    blankCheck("format"),
    blankCheck("test"),
    blankCheck("functional"),
  ]);
  const [negative, setNegative] = useState<CheckDraft[]>([
    blankCheck("negative"),
  ]);
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const loadRefs = useCallback(async () => {
    if (project.registrationStatus !== "registered") {
      return;
    }
    try {
      const refs = await api.projectRefs(project.id);
      setBaseTips(refs.baseRefs);
      const first = Object.values(refs.baseRefs)[0];
      if (first) {
        setBaseRevision((current) => current || first);
      }
    } catch (e) {
      setError(
        e instanceof ApiError ? e.message : "Failed to load base revisions.",
      );
    }
  }, [project.id, project.registrationStatus]);

  useEffect(() => {
    void loadRefs();
  }, [loadRefs]);

  useEffect(() => {
    setProfile(project.qualityProfile || "node-basic");
  }, [project.qualityProfile]);

  const requiredCapabilities = useMemo<Capability[]>(() => {
    const base: Capability[] = ["repository-edit"];
    if (profile.startsWith("node")) {
      base.push("node-toolchain");
    }
    if (profile === "node-browser") {
      base.push("browser");
    }
    if (profile === "generic") {
      base.push("generic-toolchain");
    }
    return base;
  }, [profile]);

  const missingCapabilities = requiredCapabilities.filter(
    (capability) => !project.capabilities.includes(capability),
  );

  function updateCheck(
    setter: React.Dispatch<React.SetStateAction<CheckDraft[]>>,
    index: number,
    patch: Partial<CheckDraft>,
  ) {
    setter((current) =>
      current.map((item, i) => (i === index ? { ...item, ...patch } : item)),
    );
  }

  function renderChecks(
    checks: CheckDraft[],
    setter: React.Dispatch<React.SetStateAction<CheckDraft[]>>,
    negativeList: boolean,
    testPrefix: string,
  ) {
    return checks.map((check, index) => (
      <div className="check-card" key={check.uid}>
        <div className="grid-2">
          <label>
            Check ID
            <input
              data-testid={`${testPrefix}-${index}-id`}
              value={check.id}
              onChange={(e) =>
                updateCheck(setter, index, { id: e.target.value })
              }
              placeholder={negativeList ? "negative" : "functional"}
              required
            />
          </label>
          <label>
            Kind
            <select
              data-testid={`${testPrefix}-${index}-kind`}
              value={check.kind}
              onChange={(e) =>
                updateCheck(setter, index, { kind: e.target.value })
              }
            >
              {(negativeList
                ? ["negative"]
                : ["format", "lint", "build", "test", "functional", "browser"]
              ).map((kind) => (
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
            data-testid={`${testPrefix}-${index}-description`}
            value={check.description}
            onChange={(e) =>
              updateCheck(setter, index, { description: e.target.value })
            }
            required
          />
        </label>
        <label>
          Command argv — one argument per line, never a shell string
          <textarea
            data-testid={`${testPrefix}-${index}-command`}
            value={check.command}
            onChange={(e) =>
              updateCheck(setter, index, { command: e.target.value })
            }
            rows={4}
            placeholder={"npm\nrun\nformat"}
            required
          />
        </label>
        <div className="grid-2">
          <label>
            Expected exit
            <input
              type="number"
              value={check.expectExit}
              min={0}
              max={127}
              onChange={(e) =>
                updateCheck(setter, index, {
                  expectExit: Number(e.target.value),
                })
              }
            />
          </label>
          <label>
            Timeout (seconds)
            <input
              type="number"
              value={check.timeoutSecs}
              min={1}
              max={600}
              onChange={(e) =>
                updateCheck(setter, index, {
                  timeoutSecs: Number(e.target.value),
                })
              }
            />
          </label>
        </div>
        <button
          type="button"
          className="linklike"
          onClick={() =>
            setter((current) => current.filter((_, i) => i !== index))
          }
        >
          Remove check
        </button>
      </div>
    ));
  }

  async function onSubmit(e: React.FormEvent) {
    e.preventDefault();
    setCreating(true);
    setError(null);
    setNotice(null);
    try {
      const route = ROUTES[routeIndex];
      const acceptanceChecks = acceptance.map(toCheck);
      const negativeChecks = negative.map(toCheck);
      const spec = await api.createActionSpec({
        projectId: action.projectId,
        actionId: action.id,
        goal,
        baseRevision,
        allowedPaths: allowedPaths
          .split(",")
          .map((entry) => entry.trim())
          .filter(Boolean),
        acceptanceCriteria: acceptanceChecks,
        negativeCases: negativeChecks,
        riskClass: risk,
        qualityProfile: profile,
        budget: { maxAttempts, timeoutSecs },
        executionMode: "trusted-local",
        provider: route.provider,
        model: route.model,
        paseoMode: route.mode,
      });
      setNotice(
        `Frozen ${spec.id} at content ${spec.contentHash.slice(0, 12)}; project binding ${spec.bindingHash?.slice(0, 12) ?? "legacy"}.`,
      );
      onFrozen(spec);
      setGoal("");
    } catch (err) {
      setError(
        err instanceof ApiError ? err.message : "ActionSpec freeze failed.",
      );
    } finally {
      setCreating(false);
    }
  }

  return (
    <form className="create" onSubmit={onSubmit} data-testid="spec-form">
      <h3>Freeze an ActionSpec for {project.name}</h3>
      <p className="muted">
        Repository <code>{project.repositoryPath}</code> · identity{" "}
        <code>{project.repositoryIdentity?.slice(0, 16)}</code> · config
        revision {project.configRevision} · {project.securityLabel} (not
        isolated)
      </p>
      <label>
        Goal
        <textarea
          data-testid="spec-goal"
          value={goal}
          onChange={(e) => setGoal(e.target.value)}
          maxLength={4000}
          rows={4}
          required
        />
      </label>
      <div className="grid-2">
        <label>
          Base revision (must be an authorized base-ref tip)
          <select
            data-testid="spec-base"
            value={baseRevision}
            onChange={(e) => setBaseRevision(e.target.value)}
            required
          >
            <option value="">Select a base revision…</option>
            {Object.entries(baseTips).map(([ref, sha]) => (
              <option key={ref} value={sha}>
                {ref} · {sha}
              </option>
            ))}
            {baseRevision &&
              !Object.values(baseTips).includes(baseRevision) && (
                <option value={baseRevision}>{baseRevision}</option>
              )}
          </select>
        </label>
        <label>
          Allowed repository paths (comma separated)
          <input
            data-testid="spec-paths"
            value={allowedPaths}
            onChange={(e) => setAllowedPaths(e.target.value)}
            placeholder="src, package.json"
            required
          />
        </label>
      </div>
      <div className="grid-2">
        <label>
          Quality Profile
          <select
            data-testid="spec-profile"
            value={profile}
            onChange={(e) => setProfile(e.target.value)}
          >
            {PROFILES.map((value) => (
              <option key={value} value={value}>
                {value}
              </option>
            ))}
          </select>
        </label>
        <label>
          Risk class
          <select
            data-testid="spec-risk"
            value={risk}
            onChange={(e) => setRisk(e.target.value)}
          >
            <option value="R0">R0 — low</option>
            <option value="R1">R1 — moderate</option>
          </select>
        </label>
      </div>
      <p className="muted" data-testid="spec-capabilities">
        Required capabilities: {requiredCapabilities.join(", ")}
        {missingCapabilities.length > 0 &&
          ` — PROJECT DOES NOT GRANT: ${missingCapabilities.join(", ")}`}
      </p>
      <label>
        Pinned model route
        <select
          value={routeIndex}
          onChange={(e) => setRouteIndex(Number(e.target.value))}
        >
          {ROUTES.map((route, index) => (
            <option key={route.label} value={index}>
              {route.label}
            </option>
          ))}
        </select>
      </label>
      <div className="grid-2">
        <label>
          Maximum Implementer attempts
          <input
            type="number"
            min={1}
            max={5}
            value={maxAttempts}
            onChange={(e) => setMaxAttempts(Number(e.target.value))}
          />
        </label>
        <label>
          Attempt timeout (seconds)
          <input
            type="number"
            min={30}
            max={600}
            value={timeoutSecs}
            onChange={(e) => setTimeoutSecs(Number(e.target.value))}
          />
        </label>
      </div>
      <div className="row">
        <button
          type="button"
          onClick={() => {
            const template = templateChecks(profile);
            setAcceptance(template.acceptance);
            setNegative(template.negative);
          }}
        >
          Apply visible {profile} command template
        </button>
        <span className="muted">
          Templates only fill standard commands; you must supply the
          task-specific functional and negative assertions.
        </span>
      </div>
      <section>
        <h4>Acceptance criteria</h4>
        {renderChecks(acceptance, setAcceptance, false, "acceptance")}
        <button
          type="button"
          onClick={() => setAcceptance((c) => [...c, blankCheck("functional")])}
        >
          Add acceptance check
        </button>
      </section>
      <section>
        <h4>Negative cases</h4>
        {renderChecks(negative, setNegative, true, "negative")}
        <button
          type="button"
          onClick={() => setNegative((c) => [...c, blankCheck("negative")])}
        >
          Add negative check
        </button>
      </section>
      {error && <p className="error">{error}</p>}
      {notice && <p className="notice">{notice}</p>}
      <button
        type="submit"
        disabled={creating || missingCapabilities.length > 0}
        data-testid="freeze-spec"
      >
        {creating ? "Freezing…" : "Freeze ActionSpec"}
      </button>
    </form>
  );
}
