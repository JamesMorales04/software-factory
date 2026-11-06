import { useCallback, useEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";
import {
  type Action,
  CoreApiError,
  coreApi,
  type DelegationProjection,
  type LeadAssessmentView,
  type LeadPlanProposalView,
  type LeadRequestProjection,
  type LeadRequestRecord,
  type LeadWorkUnitView,
  type NewCampaignAuthorization,
  newIdempotencyKey,
  type Project,
} from "../coreApi";
import {
  DEFAULT_OPERATION_LIMITS,
  DEFAULT_PROGRESS_POLICY,
} from "../executionDefaults";
import { shortId } from "../format";
import { useI18n } from "../i18n";
import { selectableLeadDelegations } from "../leadDelegations";

function parseJson<T>(raw: string | undefined): T | null {
  if (!raw) return null;
  try {
    return JSON.parse(raw) as T;
  } catch {
    return null;
  }
}

function list(values: string[], t: ReturnType<typeof useI18n>["t"]): string {
  return values.length === 0 ? t("common.none") : values.join("; ");
}

function checksSummary(
  unit: LeadWorkUnitView,
  t: ReturnType<typeof useI18n>["t"],
): string {
  return t("request.plan.checksValue", {
    acceptance: unit.acceptanceCriteria.length,
    negative: unit.negativeCases.length,
  });
}

function splitValues(value: string): string[] {
  return value
    .split(",")
    .map((item) => item.trim())
    .filter((item) => item !== "");
}

function joinValues(values: string[]): string {
  return values.join(", ");
}

interface StartAuthorizationDraft {
  requestId: string;
  authorizationId: string;
  actor: string;
  rationale: string;
  paths: string;
  tools: string;
}

function buildAuthorizationDraft(
  selected: LeadRequestProjection,
  action: Action | undefined,
  proposal: LeadPlanProposalView,
): StartAuthorizationDraft {
  const paths =
    action?.spec.allowed_paths ??
    Array.from(
      new Set(proposal.workUnits.flatMap((unit) => unit.allowedPaths)),
    );
  return {
    requestId: selected.request.id,
    authorizationId: `office-lead-${selected.request.id}`,
    actor: "office-operator",
    rationale: "explicit TRUSTED_LOCAL_PROVIDER start from Office",
    paths: joinValues(paths),
    tools: "write_file, run_command",
  };
}

export function Lead() {
  const { t, formatDate, formatNumber, formatMicros } = useI18n();
  const [projects, setProjects] = useState<Project[]>([]);
  const [actions, setActions] = useState<Action[]>([]);
  const [delegations, setDelegations] = useState<DelegationProjection[]>([]);
  const [requests, setRequests] = useState<LeadRequestRecord[]>([]);
  const [selected, setSelected] = useState<LeadRequestProjection | null>(null);
  const [projectId, setProjectId] = useState("");
  const [actionId, setActionId] = useState("");
  const [delegationId, setDelegationId] = useState("");
  const [objective, setObjective] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [authorizationDraft, setAuthorizationDraft] =
    useState<StartAuthorizationDraft | null>(null);
  const key = useRef(newIdempotencyKey());

  const load = useCallback(async () => {
    const [nextProjects, nextActions, nextDelegations, nextRequests] =
      await Promise.all([
        coreApi.listProjects(),
        coreApi.listActions(),
        coreApi.listDelegations(),
        coreApi.listLeadRequests(),
      ]);
    setProjects(nextProjects);
    setActions(nextActions);
    setDelegations(nextDelegations);
    setRequests(nextRequests);
    if (!projectId && nextProjects[0]) setProjectId(nextProjects[0].id);
    if (!actionId && nextActions[0]) setActionId(nextActions[0].id);
    const firstCurrentGrant = nextDelegations.find(
      (projection) => projection.schemaVersion === 2,
    );
    if (!delegationId && firstCurrentGrant)
      setDelegationId(firstCurrentGrant.delegation.id);
  }, [projectId, actionId, delegationId]);

  useEffect(() => {
    let active = true;
    async function refresh() {
      try {
        await load();
      } catch (cause) {
        if (active) {
          setError(
            cause instanceof CoreApiError
              ? cause.message
              : t("lead.error.load"),
          );
        }
      }
    }
    void refresh();
    const timer = setInterval(() => void refresh(), 4000);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [load, t]);

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const project = projects.find((item) => item.id === projectId);
      if (!project) throw new Error(t("lead.error.selectProject"));
      const config = await coreApi.getProjectConfig(project.id);
      const request = await coreApi.createLeadRequest({
        project_id: project.id,
        action_id: actionId,
        delegation_id: delegationId,
        objective: objective.trim(),
        project_config_hash: config.configHash,
        idempotency_key: key.current,
      });
      key.current = newIdempotencyKey();
      setNotice(t("lead.notice.submitted", { id: shortId(request.id) }));
      setSelected(await coreApi.getLeadRequest(request.id));
      await load();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("lead.error.submit"));
    } finally {
      setBusy(false);
    }
  }

  async function assess(id: string) {
    setBusy(true);
    setError(null);
    try {
      await coreApi.assessLeadRequest(id);
      setSelected(await coreApi.getLeadRequest(id));
      setNotice(t("lead.notice.assessed"));
      await load();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("lead.error.assess"));
    } finally {
      setBusy(false);
    }
  }

  async function freeze(id: string) {
    setBusy(true);
    setError(null);
    try {
      await coreApi.freezeLeadRequest(id);
      setSelected(await coreApi.getLeadRequest(id));
      setNotice(t("lead.notice.frozen"));
      await load();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("lead.error.freeze"));
    } finally {
      setBusy(false);
    }
  }

  async function view(id: string) {
    setError(null);
    try {
      setSelected(await coreApi.getLeadRequest(id));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("lead.error.view"));
    }
  }

  async function start(id: string, authorization?: NewCampaignAuthorization) {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const approvalProjection =
        selected?.request.id === id
          ? selected
          : await coreApi.getLeadRequest(id);
      setSelected(approvalProjection);
      if (!approvalProjection.plan || !approvalProjection.start_binding) {
        throw new Error(t("lead.error.view"));
      }
      const binding = approvalProjection.start_binding;
      const result = await coreApi.approveLeadRequest(id, {
        expected_request_hash: binding.request_hash,
        expected_plan_hash: binding.plan_hash,
        expected_spec_sha256: binding.spec_sha256,
        expected_config_hash: binding.config_hash,
        expected_delegation_hash: binding.delegation_hash,
        ...(binding.profile_revision_id === null ||
        binding.profile_hash === null
          ? {}
          : {
              profile_revision_id: binding.profile_revision_id,
              profile_hash: binding.profile_hash,
            }),
        rationale:
          authorization?.rationale ??
          "Explicit operator approval of the frozen Lead plan from Office.",
        ...(authorization === undefined
          ? {}
          : { provider_authorization: authorization }),
      });
      setAuthorizationDraft(null);
      setSelected(await coreApi.getLeadRequest(id));
      setNotice(
        t("lead.notice.started", {
          id: shortId(result.campaign.id),
          status: result.campaign.status,
        }),
      );
      await load();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("lead.error.start"));
    } finally {
      setBusy(false);
    }
  }

  function updateDraft(patch: Partial<StartAuthorizationDraft>) {
    setAuthorizationDraft((current) =>
      current === null ? current : { ...current, ...patch },
    );
  }

  async function startWithAuthorization(event: React.FormEvent) {
    event.preventDefault();
    const draft = authorizationDraft;
    if (!draft || !selected) return;
    try {
      if (!selectedProposal) {
        throw new Error(t("lead.error.proposalUnavailable"));
      }
      const authorization: NewCampaignAuthorization = {
        authorizationId: draft.authorizationId.trim(),
        riskClass: "R0",
        executionMode: "TRUSTED_LOCAL_PROVIDER",
        actor: draft.actor.trim(),
        rationale: draft.rationale.trim(),
        authorizedPaths: splitValues(draft.paths),
        provider: selectedProposal?.route.provider ?? "",
        model: selectedProposal?.route.model ?? "",
        providerMode: selectedProposal?.route.mode ?? "",
        delegationId: selected.request.delegation_id,
        allowedTools: splitValues(draft.tools),
        operations: { ...DEFAULT_OPERATION_LIMITS },
        progress: { ...DEFAULT_PROGRESS_POLICY },
      };
      if (authorization.authorizedPaths.length === 0) {
        throw new Error(t("lead.error.pathsEmpty"));
      }
      await start(draft.requestId, authorization);
    } catch (cause) {
      setError(
        cause instanceof Error
          ? cause.message
          : t("lead.error.authorizationIncomplete"),
      );
    }
  }

  const projectActions = actions.filter(
    (action) => action.project_id === projectId,
  );
  const projectDelegations = selectableLeadDelegations(delegations, projectId);
  const selectedAssessment = selected
    ? parseJson<LeadAssessmentView>(selected.assessment?.assessment_json)
    : null;
  const selectedProposal = selected
    ? parseJson<LeadPlanProposalView>(selected.proposal?.proposal_json)
    : null;

  useEffect(() => {
    if (!selected) {
      setAuthorizationDraft(null);
      return;
    }
    if (
      selected.request.state !== "READY" ||
      selected.request.assessment_mechanism !== "MODEL"
    ) {
      setAuthorizationDraft(null);
      return;
    }
    const proposal = parseJson<LeadPlanProposalView>(
      selected.proposal?.proposal_json,
    );
    if (!proposal) {
      setAuthorizationDraft(null);
      return;
    }
    const action = actions.find(
      (item) => item.id === selected.request.action_id,
    );
    setAuthorizationDraft((current) =>
      current?.requestId === selected.request.id
        ? current
        : buildAuthorizationDraft(selected, action, proposal),
    );
  }, [selected, actions]);

  return (
    <div className="lead-workspace">
      <h2>{t("lead.title")}</h2>
      <p className="muted">
        {t("lead.description.start")} <code>TRUSTED_LOCAL_PROVIDER</code>{" "}
        {t("lead.description.end")}
      </p>
      {error && <p className="error">{error}</p>}
      {notice && <p className="notice">{notice}</p>}

      <form className="create" onSubmit={submit}>
        <h3>{t("lead.form.title")}</h3>
        <label>
          {t("lead.form.project")}
          <select
            data-testid="lead-project"
            value={projectId}
            onChange={(event) => {
              setProjectId(event.target.value);
              setDelegationId("");
            }}
          >
            <option value="">{t("lead.form.selectProject")}</option>
            {projects.map((project) => (
              <option key={project.id} value={project.id}>
                {shortId(project.id)} — {project.repository}
              </option>
            ))}
          </select>
        </label>
        <label>
          {t("lead.form.action")}
          <select
            data-testid="lead-action"
            value={actionId}
            onChange={(event) => setActionId(event.target.value)}
          >
            <option value="">{t("lead.form.selectAction")}</option>
            {projectActions.map((action) => (
              <option key={action.id} value={action.id}>
                {shortId(action.id)} — {action.objective}
              </option>
            ))}
          </select>
        </label>
        <label>
          {t("lead.form.delegation")}
          <select
            data-testid="lead-delegation"
            value={delegationId}
            onChange={(event) => setDelegationId(event.target.value)}
          >
            <option value="">{t("lead.form.selectDelegation")}</option>
            {projectDelegations.map((delegation) => (
              <option key={delegation.id} value={delegation.id}>
                {delegation.id} — {delegation.route.provider}/
                {delegation.route.model}
              </option>
            ))}
          </select>
        </label>
        <label>
          {t("lead.form.objective")}
          <textarea
            data-testid="lead-objective"
            value={objective}
            onChange={(event) => setObjective(event.target.value)}
            placeholder={t("lead.form.objectivePlaceholder")}
            required
          />
        </label>
        <button
          type="submit"
          disabled={
            busy ||
            !projectId ||
            !actionId ||
            !delegationId ||
            !objective.trim()
          }
        >
          {busy ? t("lead.form.submitting") : t("lead.form.submit")}
        </button>
      </form>

      <h3>{t("lead.list.title", { count: formatNumber(requests.length) })}</h3>
      {requests.length === 0 ? (
        <p className="muted">{t("lead.list.empty")}</p>
      ) : (
        <div className="table-responsive">
          <table
            aria-label={t("lead.list.title", {
              count: formatNumber(requests.length),
            })}
          >
            <thead>
              <tr>
                <th>{t("lead.table.id")}</th>
                <th>{t("lead.table.state")}</th>
                <th>{t("lead.table.assessment")}</th>
                <th>{t("lead.table.objective")}</th>
                <th>{t("lead.table.campaign")}</th>
                <th>{t("lead.table.updated")}</th>
                <th>{t("lead.table.actions")}</th>
              </tr>
            </thead>
            <tbody>
              {requests.map((request) => (
                <tr key={request.id}>
                  <td data-label={t("lead.table.id")}>
                    <code>{shortId(request.id)}</code>
                  </td>
                  <td data-label={t("lead.table.state")}>{request.state}</td>
                  <td data-label={t("lead.table.assessment")}>
                    {request.assessment_mechanism ?? "—"}
                  </td>
                  <td data-label={t("lead.table.objective")}>
                    {request.objective}
                  </td>
                  <td data-label={t("lead.table.campaign")}>
                    {request.campaign_id ? (
                      <Link to={`/campaigns/${request.campaign_id}`}>
                        <code>{shortId(request.campaign_id)}</code>
                      </Link>
                    ) : (
                      <span className="muted">—</span>
                    )}
                  </td>
                  <td className="muted" data-label={t("lead.table.updated")}>
                    {formatDate(request.updated_at)}
                  </td>
                  <td data-label={t("lead.table.actions")}>
                    <button type="button" onClick={() => void view(request.id)}>
                      {t("lead.view")}
                    </button>
                    {["SUBMITTED", "ASSESSING"].includes(request.state) && (
                      <button
                        type="button"
                        disabled={busy}
                        onClick={() => void assess(request.id)}
                        title={t("lead.assessHint")}
                      >
                        {t("lead.assess")}
                      </button>
                    )}
                    {request.state === "PLANNED" && (
                      <button
                        type="button"
                        disabled={busy}
                        onClick={() => void freeze(request.id)}
                      >
                        {t("lead.freezePlan")}
                      </button>
                    )}
                    {request.state === "READY" &&
                      request.assessment_mechanism === "FIXTURE" && (
                        <button
                          type="button"
                          disabled={busy}
                          onClick={() => void start(request.id)}
                        >
                          {t("lead.startCampaign")}
                        </button>
                      )}
                    {request.state === "READY" &&
                      request.assessment_mechanism === "MODEL" && (
                        <button
                          type="button"
                          disabled={busy}
                          onClick={() => void view(request.id)}
                        >
                          {t("lead.authorizeStart")}
                        </button>
                      )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {selected && (
        <section data-testid="lead-selected">
          <h3>{t("lead.selected.title")}</h3>
          <div className="view-head">
            <p>
              {t("lead.selected.stateLabel")}{" "}
              <strong>{selected.request.state}</strong> ·{" "}
              {t("lead.selected.meta", {
                created: formatDate(selected.request.created_at),
                updated: formatDate(selected.request.updated_at),
              })}
            </p>
            {selected.request.campaign_id && (
              <Link to={`/campaigns/${selected.request.campaign_id}`}>
                {t("lead.openCampaign", {
                  id: shortId(selected.request.campaign_id),
                })}
              </Link>
            )}
          </div>
          <dl className="detail">
            <dt>{t("lead.fact.request")}</dt>
            <dd>
              <code>{shortId(selected.request.id)}</code>
            </dd>
            <dt>{t("lead.fact.project")}</dt>
            <dd>
              <code>{shortId(selected.request.project_id)}</code>
            </dd>
            <dt>{t("lead.fact.action")}</dt>
            <dd>
              <code>{shortId(selected.request.action_id)}</code>
            </dd>
            <dt>{t("lead.fact.delegation")}</dt>
            <dd>
              <code>{selected.request.delegation_id}</code>
            </dd>
            <dt>{t("lead.fact.configHash")}</dt>
            <dd>
              <code title={selected.request.project_config_hash}>
                {shortId(selected.request.project_config_hash, 12)}
              </code>
            </dd>
          </dl>
          {selected.request.failure_reason && (
            <p className="error">{selected.request.failure_reason}</p>
          )}

          <h4>{t("lead.assessment.title")}</h4>
          {selected.assessment ? (
            <>
              <p className="muted">
                {t("lead.assessment.meta", {
                  mechanism: selected.assessment.mechanism,
                  date: formatDate(selected.assessment.created_at),
                })}{" "}
                <code>{shortId(selected.assessment.schema_sha256, 12)}</code>
              </p>
              {selected.assessment.mechanism === "MODEL" ? (
                <p className="warning">
                  {t("lead.assessment.modelWarning")}{" "}
                  <code>TRUSTED_LOCAL_PROVIDER</code>{" "}
                  {t("lead.assessment.modelWarningEnd")}
                </p>
              ) : (
                <p className="muted">{t("lead.assessment.fixtureNote")}</p>
              )}
              {selectedAssessment ? (
                <dl className="detail" data-testid="lead-assessment">
                  <dt>{t("request.assessment.intent")}</dt>
                  <dd>{selectedAssessment.intent}</dd>
                  <dt>{t("request.assessment.complexity")}</dt>
                  <dd>{selectedAssessment.complexity}</dd>
                  <dt>{t("lead.assessment.effort")}</dt>
                  <dd>
                    {selectedAssessment.effortEstimate.status === "estimated"
                      ? `${selectedAssessment.effortEstimate.minPersonHours}–${selectedAssessment.effortEstimate.maxPersonHours} ${selectedAssessment.effortEstimate.unit}`
                      : `${selectedAssessment.effortEstimate.status.replaceAll("_", " ")}${
                          selectedAssessment.effortEstimate.unknownReason
                            ? `: ${selectedAssessment.effortEstimate.unknownReason}`
                            : ""
                        }`}
                  </dd>
                  <dt>{t("lead.assessment.evidenceRefs")}</dt>
                  <dd>{list(selectedAssessment.evidenceRefs, t)}</dd>
                  <dt>{t("lead.assessment.changeImpact")}</dt>
                  <dd>{list(selectedAssessment.changeImpact, t)}</dd>
                  <dt>{t("lead.assessment.validationNeeds")}</dt>
                  <dd>{list(selectedAssessment.validationNeeds, t)}</dd>
                  <dt>{t("lead.assessment.knowledgeGaps")}</dt>
                  <dd>{list(selectedAssessment.knowledgeGaps, t)}</dd>
                  <dt>{t("lead.assessment.assumptions")}</dt>
                  <dd>{list(selectedAssessment.assumptions, t)}</dd>
                  <dt>{t("lead.assessment.uncertainty")}</dt>
                  <dd>{list(selectedAssessment.uncertainty, t)}</dd>
                  <dt>{t("lead.assessment.suggestedNextAction")}</dt>
                  <dd>{selectedAssessment.suggestedNextAction}</dd>
                </dl>
              ) : (
                <p className="error">{t("lead.assessment.decodeError")}</p>
              )}
            </>
          ) : (
            <p className="muted">{t("lead.assessment.none")}</p>
          )}

          {(selected.model_calls.length > 0 ||
            selected.assessment?.mechanism === "MODEL") && (
            <>
              <h4>{t("lead.modelCalls.title")}</h4>
              {selected.model_calls.length === 0 ? (
                <p className="muted">{t("lead.modelCalls.none")}</p>
              ) : (
                <div className="table-responsive">
                  <table
                    data-testid="lead-model-calls"
                    aria-label={t("lead.modelCalls.title")}
                  >
                    <thead>
                      <tr>
                        <th>{t("lead.modelCalls.providerModel")}</th>
                        <th>{t("lead.modelCalls.reservation")}</th>
                        <th>{t("lead.modelCalls.state")}</th>
                        <th>{t("lead.modelCalls.usage")}</th>
                        <th>{t("lead.modelCalls.purpose")}</th>
                        <th>{t("lead.modelCalls.recorded")}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {selected.model_calls.map((call) => (
                        <tr key={call.reservation_id}>
                          <td data-label={t("lead.modelCalls.providerModel")}>
                            {call.provider}/{call.model}
                          </td>
                          <td data-label={t("lead.modelCalls.reservation")}>
                            <code>{shortId(call.reservation_id)}</code>
                          </td>
                          <td data-label={t("lead.modelCalls.state")}>
                            {call.state}
                          </td>
                          <td data-label={t("lead.modelCalls.usage")}>
                            {call.usage_known ? (
                              <>
                                {t("sessions.usage.inputOutput", {
                                  input: formatNumber(call.input_tokens),
                                  output: formatNumber(call.output_tokens),
                                })}
                                <div className="muted">
                                  {t("sessions.usage.total", {
                                    tokens: formatNumber(call.total_tokens),
                                    cost: formatMicros(call.cost_micros),
                                  })}
                                </div>
                              </>
                            ) : (
                              <span className="muted">UNMEASURED</span>
                            )}
                          </td>
                          <td data-label={t("lead.modelCalls.purpose")}>
                            {call.purpose}
                          </td>
                          <td
                            className="muted"
                            data-label={t("lead.modelCalls.recorded")}
                          >
                            {formatDate(call.created_at)}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </>
          )}

          <h4>{t("lead.proposal.title")}</h4>
          {selected.proposal ? (
            <>
              <p className="muted">
                {t("lead.proposal.metaLabel")}{" "}
                <code>{shortId(selected.proposal.id)}</code> ·{" "}
                {t("lead.proposal.metaHash")}{" "}
                <code>{shortId(selected.proposal.proposal_hash, 12)}</code> ·{" "}
                {t("lead.proposal.metaRecorded", {
                  date: formatDate(selected.proposal.created_at),
                })}
              </p>
              {selectedProposal ? (
                <div data-testid="lead-proposal">
                  <dl className="detail">
                    <dt>{t("lead.proposal.classRisk")}</dt>
                    <dd>
                      {selectedProposal.class} / {selectedProposal.riskClass}
                    </dd>
                    <dt>{t("lead.proposal.qualityProfile")}</dt>
                    <dd>{selectedProposal.qualityProfile}</dd>
                    <dt>{t("lead.proposal.route")}</dt>
                    <dd>
                      {selectedProposal.route.provider}/
                      {selectedProposal.route.model} (
                      {selectedProposal.route.mode})
                    </dd>
                    <dt>{t("lead.proposal.baseRevision")}</dt>
                    <dd>
                      <code title={selectedProposal.baseRevision}>
                        {shortId(selectedProposal.baseRevision, 12)}
                      </code>
                    </dd>
                    <dt>{t("lead.proposal.budget")}</dt>
                    <dd>
                      {t("lead.proposal.budgetValue", {
                        attempts: selectedProposal.budget.maxAttempts,
                        timeout: selectedProposal.budget.timeoutSecs,
                        repairCycles: selectedProposal.maxRepairCycles,
                        sessions: selectedProposal.maxSessions,
                      })}
                    </dd>
                    <dt>{t("lead.proposal.parallel")}</dt>
                    <dd>
                      {t("lead.proposal.parallelValue", {
                        parallel: selectedProposal.maxParallel,
                        wallClock: selectedProposal.maxWallClockSecs,
                      })}
                    </dd>
                    <dt>{t("lead.proposal.assumptions")}</dt>
                    <dd>{list(selectedProposal.assumptions, t)}</dd>
                    <dt>{t("lead.proposal.risks")}</dt>
                    <dd>{list(selectedProposal.risks, t)}</dd>
                  </dl>
                  <h5>
                    {t("lead.proposal.workUnits", {
                      count: formatNumber(selectedProposal.workUnits.length),
                    })}
                  </h5>
                  <div className="table-responsive">
                    <table
                      aria-label={t("lead.proposal.workUnits", {
                        count: formatNumber(selectedProposal.workUnits.length),
                      })}
                    >
                      <thead>
                        <tr>
                          <th>{t("lead.proposal.table.index")}</th>
                          <th>{t("lead.proposal.table.unit")}</th>
                          <th>{t("lead.proposal.table.kind")}</th>
                          <th>{t("lead.proposal.table.dependsOn")}</th>
                          <th>{t("lead.proposal.table.paths")}</th>
                          <th>{t("lead.proposal.table.checks")}</th>
                        </tr>
                      </thead>
                      <tbody>
                        {selectedProposal.workUnits.map((unit, index) => (
                          <tr key={unit.id}>
                            <td data-label={t("lead.proposal.table.index")}>
                              {index + 1}
                            </td>
                            <td data-label={t("lead.proposal.table.unit")}>
                              <code>{unit.id}</code>
                              <div className="muted">{unit.description}</div>
                            </td>
                            <td data-label={t("lead.proposal.table.kind")}>
                              {unit.kind}
                            </td>
                            <td data-label={t("lead.proposal.table.dependsOn")}>
                              {unit.dependsOn.join(", ") || t("common.none")}
                            </td>
                            <td data-label={t("lead.proposal.table.paths")}>
                              {unit.allowedPaths.join(", ") || t("common.none")}
                              <div className="muted">
                                {t("lead.proposal.capabilities")}{" "}
                                {unit.capabilities.join(", ") ||
                                  t("common.none")}
                              </div>
                            </td>
                            <td data-label={t("lead.proposal.table.checks")}>
                              {checksSummary(unit, t)}
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                </div>
              ) : (
                <p className="error">{t("lead.proposal.decodeError")}</p>
              )}
            </>
          ) : (
            <p className="muted">{t("lead.proposal.none")}</p>
          )}

          <h4>{t("lead.plan.title")}</h4>
          {selected.plan ? (
            <dl className="detail" data-testid="lead-plan">
              <dt>{t("lead.plan.plan")}</dt>
              <dd>
                <code>{shortId(selected.plan.id)}</code>
              </dd>
              <dt>{t("lead.plan.planHash")}</dt>
              <dd>
                <code title={selected.plan.plan_hash}>
                  {shortId(selected.plan.plan_hash, 12)}
                </code>
              </dd>
              <dt>{t("lead.plan.configHash")}</dt>
              <dd>
                <code title={selected.plan.config_hash}>
                  {shortId(selected.plan.config_hash, 12)}
                </code>
              </dd>
              <dt>{t("lead.plan.delegationHash")}</dt>
              <dd>
                <code title={selected.plan.delegation_hash}>
                  {shortId(selected.plan.delegation_hash, 12)}
                </code>
              </dd>
              <dt>{t("lead.plan.proposal")}</dt>
              <dd>
                <code>{shortId(selected.plan.proposal_id)}</code>
              </dd>
              <dt>{t("lead.plan.frozen")}</dt>
              <dd>{formatDate(selected.plan.created_at)}</dd>
            </dl>
          ) : (
            <p className="muted">{t("lead.plan.none")}</p>
          )}

          {selected.request.state === "READY" && (
            <>
              <h4>{t("lead.start.title")}</h4>
              <p className="muted">{t("lead.start.policyNote")}</p>
              {selected.request.assessment_mechanism === "FIXTURE" ? (
                <>
                  <p className="muted">{t("lead.start.fixtureNote")}</p>
                  <button
                    type="button"
                    data-testid="lead-start-fixture"
                    disabled={busy}
                    onClick={() => void start(selected.request.id)}
                  >
                    {busy ? t("lead.start.starting") : t("lead.startCampaign")}
                  </button>
                </>
              ) : selected.request.assessment_mechanism === "MODEL" ? (
                authorizationDraft?.requestId === selected.request.id ? (
                  <form
                    className="create"
                    data-testid="lead-start-authorization"
                    onSubmit={(event) => void startWithAuthorization(event)}
                  >
                    <p className="warning">{t("lead.start.modelWarning")}</p>
                    <label>
                      {t("lead.start.authorizationId")}
                      <input
                        data-testid="lead-start-authorization-id"
                        value={authorizationDraft.authorizationId}
                        onChange={(event) =>
                          updateDraft({
                            authorizationId: event.target.value,
                          })
                        }
                        required
                      />
                    </label>
                    <label>
                      {t("lead.start.actor")}
                      <input
                        value={authorizationDraft.actor}
                        onChange={(event) =>
                          updateDraft({ actor: event.target.value })
                        }
                        required
                      />
                    </label>
                    <label>
                      {t("lead.start.rationale")}
                      <input
                        value={authorizationDraft.rationale}
                        onChange={(event) =>
                          updateDraft({ rationale: event.target.value })
                        }
                        required
                      />
                    </label>
                    <label>
                      {t("lead.start.paths")}
                      <input
                        value={authorizationDraft.paths}
                        onChange={(event) =>
                          updateDraft({ paths: event.target.value })
                        }
                        required
                      />
                    </label>
                    <p className="muted">
                      {t("lead.proposal.route")}{" "}
                      <code>
                        {selectedProposal?.route.provider}/
                        {selectedProposal?.route.model} (
                        {selectedProposal?.route.mode})
                      </code>{" "}
                      · {t("lead.fact.delegation")}{" "}
                      <code>{selected.request.delegation_id}</code> ·{" "}
                      {t("lead.start.risk")} R0
                    </p>
                    <p className="muted">{t("lead.start.technicalLimits")}</p>
                    <label>
                      {t("lead.start.tools")}
                      <input
                        value={authorizationDraft.tools}
                        onChange={(event) =>
                          updateDraft({ tools: event.target.value })
                        }
                      />
                    </label>
                    <button type="submit" disabled={busy}>
                      {busy
                        ? t("lead.start.authorizing")
                        : t("lead.start.authorize")}
                    </button>
                  </form>
                ) : (
                  <p className="muted">{t("lead.start.loading")}</p>
                )
              ) : (
                <p className="warning">
                  {t("lead.start.mechanismUnavailable")}
                </p>
              )}
            </>
          )}

          {selected.assessment && (
            <details>
              <summary>{t("lead.rawAssessment")}</summary>
              <pre>{selected.assessment.assessment_json}</pre>
            </details>
          )}
          {selected.proposal && (
            <details>
              <summary>{t("lead.rawProposal")}</summary>
              <pre>{selected.proposal.proposal_json}</pre>
            </details>
          )}
          <p className="muted">{t("lead.footnote")}</p>
        </section>
      )}
    </div>
  );
}
