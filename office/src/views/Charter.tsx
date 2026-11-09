import { useCallback, useEffect, useState } from "react";
import { Badge } from "../components/ui/Badge";
import { Button } from "../components/ui/Button";
import { Checkbox, Input, Select } from "../components/ui/Field";
import {
  EmptyState,
  ErrorState,
  LoadingState,
  PageHeader,
  Panel,
} from "../components/ui/Surface";
import {
  type AgenticCharterRecord,
  type AgenticOwnerDecision,
  CoreApiError,
  coreApi,
  type Project,
} from "../coreApi";
import { useI18n } from "../i18n";
import "./charter.css";

const RESERVED = [
  "credentials",
  "isolation",
  "production",
  "destructive_migrations",
  "public_contracts",
  "security_exceptions",
  "policy_changes",
  "out_of_budget",
  "cross_project_effects",
  "delivery",
] as const;

function errorText(cause: unknown, fallback: string): string {
  return cause instanceof CoreApiError ? cause.message : fallback;
}

function defaultExpiry(): string {
  const date = new Date(Date.now() + 30 * 24 * 60 * 60 * 1000);
  return date.toISOString().slice(0, 10);
}

export function Charter() {
  const { t, formatDate } = useI18n();
  const [projects, setProjects] = useState<Project[]>([]);
  const [charters, setCharters] = useState<AgenticCharterRecord[] | null>(null);
  const [decisions, setDecisions] = useState<AgenticOwnerDecision[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [projectId, setProjectId] = useState("");
  const [domains, setDomains] = useState("repository");
  const [risk, setRisk] = useState<"R0" | "R1" | "R2" | "R3">("R0");
  const [execution, setExecution] = useState(false);
  const [repair, setRepair] = useState(false);
  const [delivery, setDelivery] = useState(false);
  const [reserved, setReserved] = useState<string[]>([...RESERVED]);
  const [expiresAt, setExpiresAt] = useState(defaultExpiry);
  const [revokeReasons, setRevokeReasons] = useState<Record<string, string>>(
    {},
  );
  const [decisionOptions, setDecisionOptions] = useState<
    Record<string, string>
  >({});
  const [decisionRationales, setDecisionRationales] = useState<
    Record<string, string>
  >({});
  const [decisionMatter, setDecisionMatter] = useState<string>(RESERVED[0]);
  const [decisionRisk, setDecisionRisk] = useState<"R0" | "R1" | "R2" | "R3">(
    "R2",
  );
  const [decisionNote, setDecisionNote] = useState("");

  const load = useCallback(async () => {
    try {
      const [charterResult, decisionResult, projectResult] = await Promise.all([
        coreApi.listAgenticCharters(),
        coreApi.listAgenticDecisions(undefined, "PENDING"),
        coreApi.listProjects(),
      ]);
      setCharters(charterResult.charters);
      setDecisions(decisionResult.decisions);
      setProjects(projectResult);
      if (!projectId && projectResult.length > 0) {
        setProjectId(projectResult[0].id);
      }
      setError(null);
    } catch (cause) {
      setError(errorText(cause, t("charter.error.load")));
    }
  }, [projectId, t]);

  useEffect(() => {
    void load();
  }, [load]);

  function toggleReserved(matter: string, checked: boolean) {
    setReserved((current) =>
      checked
        ? [...new Set([...current, matter])]
        : current.filter((item) => item !== matter),
    );
  }

  async function create() {
    if (!projectId || !domains.trim() || !expiresAt) {
      setActionError(t("charter.create.required"));
      return;
    }
    setBusy("create");
    setActionError(null);
    setNotice(null);
    try {
      await coreApi.createAgenticCharter({
        project_id: projectId,
        domains: domains
          .split(",")
          .map((domain) => domain.trim())
          .filter(Boolean),
        risk_appetite: risk,
        execution_autonomy: execution,
        repair_autonomy: repair,
        delivery_autonomy: delivery,
        reserved_matters: reserved,
        expires_at: new Date(`${expiresAt}T23:59:59Z`).toISOString(),
      });
      await load();
      setNotice(t("charter.create.done"));
    } catch (cause) {
      setActionError(errorText(cause, t("charter.create.error")));
    } finally {
      setBusy(null);
    }
  }

  async function revoke(charter: AgenticCharterRecord) {
    const reason = revokeReasons[charter.id]?.trim();
    if (!reason) {
      setActionError(t("charter.revoke.required"));
      return;
    }
    setBusy(`revoke:${charter.id}`);
    setActionError(null);
    try {
      await coreApi.revokeAgenticCharter(charter.id, reason);
      await load();
      setNotice(t("charter.revoke.done"));
    } catch (cause) {
      setActionError(errorText(cause, t("charter.revoke.error")));
    } finally {
      setBusy(null);
    }
  }

  async function openDecision() {
    if (!projectId || !decisionNote.trim()) {
      setActionError(t("charter.pending.required"));
      return;
    }
    setBusy("decision-create");
    setActionError(null);
    setNotice(null);
    try {
      await coreApi.createAgenticOwnerDecision({
        project_id: projectId,
        matter: decisionMatter,
        risk: decisionRisk,
        options: ["APPROVE", "DENY"],
        evidence: { note: decisionNote.trim() },
        recommendation: "DENY",
        deadline: new Date(Date.now() + 24 * 60 * 60 * 1000).toISOString(),
      });
      setDecisionNote("");
      await load();
      setNotice(t("charter.pending.created"));
    } catch (cause) {
      setActionError(errorText(cause, t("charter.pending.error")));
    } finally {
      setBusy(null);
    }
  }

  async function resolve(decision: AgenticOwnerDecision) {
    const option = decisionOptions[decision.id] ?? "";
    const rationale = decisionRationales[decision.id]?.trim();
    if (!option || !rationale) {
      setActionError(t("charter.decisions.required"));
      return;
    }
    setBusy(`decision:${decision.id}`);
    setActionError(null);
    try {
      await coreApi.resolveAgenticOwnerDecision(decision.id, option, rationale);
      await load();
      setNotice(t("charter.decisions.done"));
    } catch (cause) {
      setActionError(errorText(cause, t("charter.decisions.error")));
    } finally {
      setBusy(null);
    }
  }

  if (error) {
    return (
      <div className="page">
        <PageHeader
          title={t("charter.title")}
          description={t("charter.description")}
        />
        <ErrorState
          message={error}
          action={
            <Button variant="secondary" onClick={() => void load()}>
              {t("charter.retry")}
            </Button>
          }
        />
      </div>
    );
  }
  if (!charters) {
    return (
      <div className="page">
        <PageHeader
          title={t("charter.title")}
          description={t("charter.description")}
        />
        <LoadingState label={t("charter.loading")} />
      </div>
    );
  }

  return (
    <div className="page">
      <PageHeader
        title={t("charter.title")}
        description={t("charter.description")}
      />
      {actionError && (
        <p className="ui-callout ui-callout--danger" role="alert">
          {actionError}
        </p>
      )}
      {notice && (
        <p className="notice" role="status">
          {notice}
        </p>
      )}

      <Panel>
        <div className="section-heading">
          <h2>{t("charter.pending.title")}</h2>
          <Badge tone={decisions.length > 0 ? "attention" : "neutral"}>
            {decisions.length}
          </Badge>
        </div>
        {decisions.length === 0 ? (
          <p className="muted">{t("charter.pending.empty")}</p>
        ) : (
          <ul className="charter-decisions">
            {decisions.map((decision) => (
              <li key={decision.id}>
                <div className="charter-decision-heading">
                  <strong>{decision.matter}</strong>
                  <Badge tone="attention">{decision.risk}</Badge>
                  <span className="muted">
                    {t("charter.deadline", {
                      date: formatDate(decision.deadline),
                    })}
                  </span>
                </div>
                <pre className="charter-evidence">
                  {JSON.stringify(decision.evidence, null, 2)}
                </pre>
                <p className="muted">
                  {t("charter.recommendation", {
                    value: decision.recommendation,
                  })}
                </p>
                <div className="charter-decision-actions">
                  <Select
                    label={t("charter.decisions.option")}
                    value={
                      decisionOptions[decision.id] ?? decision.options[0] ?? ""
                    }
                    onChange={(event) =>
                      setDecisionOptions((current) => ({
                        ...current,
                        [decision.id]: event.target.value,
                      }))
                    }
                  >
                    <option value="">{t("charter.decisions.choose")}</option>
                    {decision.options.map((option) => (
                      <option key={option} value={option}>
                        {option}
                      </option>
                    ))}
                  </Select>
                  <Input
                    label={t("charter.decisions.rationale")}
                    value={decisionRationales[decision.id] ?? ""}
                    onChange={(event) =>
                      setDecisionRationales((current) => ({
                        ...current,
                        [decision.id]: event.target.value,
                      }))
                    }
                  />
                  <Button
                    size="sm"
                    busy={busy === `decision:${decision.id}`}
                    disabled={
                      busy !== null || !(decisionOptions[decision.id] ?? "")
                    }
                    onClick={() => void resolve(decision)}
                  >
                    {t("charter.decisions.resolve")}
                  </Button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </Panel>

      <Panel>
        <div className="section-heading">
          <h2>{t("charter.pending.createTitle")}</h2>
        </div>
        <div className="charter-form">
          <Select
            label={t("charter.pending.matter")}
            value={decisionMatter}
            onChange={(event) => setDecisionMatter(event.target.value)}
          >
            {RESERVED.map((matter) => (
              <option key={matter} value={matter}>
                {matter}
              </option>
            ))}
          </Select>
          <Select
            label={t("charter.pending.risk")}
            value={decisionRisk}
            onChange={(event) =>
              setDecisionRisk(event.target.value as "R0" | "R1" | "R2" | "R3")
            }
          >
            {["R0", "R1", "R2", "R3"].map((value) => (
              <option key={value} value={value}>
                {value}
              </option>
            ))}
          </Select>
          <Input
            label={t("charter.pending.note")}
            value={decisionNote}
            onChange={(event) => setDecisionNote(event.target.value)}
          />
          <Button
            variant="secondary"
            size="sm"
            busy={busy === "decision-create"}
            disabled={busy !== null || !projectId}
            onClick={() => void openDecision()}
          >
            {t("charter.pending.create")}
          </Button>
        </div>
        <p className="muted">{t("charter.pending.createNote")}</p>
      </Panel>

      <Panel>
        <div className="section-heading">
          <h2>{t("charter.active.title")}</h2>
          <Badge tone="neutral">{charters.length}</Badge>
        </div>
        {charters.length === 0 ? (
          <EmptyState
            title={t("charter.active.emptyTitle")}
            description={t("charter.active.emptyDescription")}
          />
        ) : (
          <ul className="charter-list">
            {charters.map((charter) => (
              <li key={charter.id}>
                <div className="charter-decision-heading">
                  <strong>{charter.projectId}</strong>
                  <Badge tone={charter.revokedAt ? "danger" : "success"}>
                    {charter.revokedAt ? "REVOKED" : "ACTIVE"}
                  </Badge>
                  <Badge tone="info">{charter.riskAppetite}</Badge>
                </div>
                <p className="muted">
                  {t("charter.domains", {
                    domains: charter.domains.join(", "),
                  })}{" "}
                  ·{" "}
                  {t("charter.expires", {
                    date: formatDate(charter.expiresAt),
                  })}
                </p>
                <p className="muted">
                  {t("charter.flags", {
                    execution: String(charter.executionAutonomy),
                    repair: String(charter.repairAutonomy),
                    delivery: String(charter.deliveryAutonomy),
                  })}
                </p>
                <p className="muted">
                  {t("charter.reserved", {
                    matters: charter.reservedMatters.join(", "),
                  })}
                </p>
                {!charter.revokedAt && (
                  <div className="charter-decision-actions">
                    <Input
                      label={t("charter.revoke.reason")}
                      value={revokeReasons[charter.id] ?? ""}
                      onChange={(event) =>
                        setRevokeReasons((current) => ({
                          ...current,
                          [charter.id]: event.target.value,
                        }))
                      }
                    />
                    <Button
                      variant="danger"
                      size="sm"
                      busy={busy === `revoke:${charter.id}`}
                      disabled={busy !== null}
                      onClick={() => void revoke(charter)}
                    >
                      {t("charter.revoke.action")}
                    </Button>
                  </div>
                )}
              </li>
            ))}
          </ul>
        )}
      </Panel>

      <Panel>
        <div className="section-heading">
          <h2>{t("charter.create.title")}</h2>
        </div>
        <div className="charter-form">
          <Select
            label={t("charter.create.project")}
            value={projectId}
            onChange={(event) => setProjectId(event.target.value)}
          >
            {projects.map((project) => (
              <option key={project.id} value={project.id}>
                {project.name || project.id}
              </option>
            ))}
          </Select>
          <Input
            label={t("charter.create.domains")}
            value={domains}
            onChange={(event) => setDomains(event.target.value)}
          />
          <Select
            label={t("charter.create.risk")}
            value={risk}
            onChange={(event) =>
              setRisk(event.target.value as "R0" | "R1" | "R2" | "R3")
            }
          >
            {["R0", "R1", "R2", "R3"].map((value) => (
              <option key={value} value={value}>
                {value}
              </option>
            ))}
          </Select>
          <Input
            label={t("charter.create.expiry")}
            type="date"
            value={expiresAt}
            onChange={(event) => setExpiresAt(event.target.value)}
          />
        </div>
        <div className="charter-checks">
          <Checkbox
            label={t("charter.create.execution")}
            checked={execution}
            onChange={(event) => setExecution(event.target.checked)}
          />
          <Checkbox
            label={t("charter.create.repair")}
            checked={repair}
            onChange={(event) => setRepair(event.target.checked)}
          />
          <Checkbox
            label={t("charter.create.delivery")}
            checked={delivery}
            onChange={(event) => setDelivery(event.target.checked)}
          />
        </div>
        <fieldset className="charter-reserved">
          <legend>{t("charter.create.reserved")}</legend>
          {RESERVED.map((matter) => (
            <Checkbox
              key={matter}
              label={matter}
              checked={reserved.includes(matter)}
              onChange={(event) => toggleReserved(matter, event.target.checked)}
            />
          ))}
        </fieldset>
        <Button
          size="sm"
          busy={busy === "create"}
          disabled={busy !== null || !projectId}
          onClick={() => void create()}
        >
          {t("charter.create.action")}
        </Button>
        <p className="muted">{t("charter.create.note")}</p>
      </Panel>
    </div>
  );
}
