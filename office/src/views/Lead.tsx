import { useCallback, useEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";
import {
  type Action,
  CoreApiError,
  coreApi,
  type LeadAssessmentView,
  type LeadPlanProposalView,
  type LeadRequestProjection,
  type LeadRequestRecord,
  type LeadWorkUnitView,
  newIdempotencyKey,
  type Project,
  type StandingDelegation,
} from "../coreApi";
import { formatTimestamp, shortId } from "../format";

function parseJson<T>(raw: string | undefined): T | null {
  if (!raw) return null;
  try {
    return JSON.parse(raw) as T;
  } catch {
    return null;
  }
}

function list(values: string[]): string {
  return values.length === 0 ? "none" : values.join("; ");
}

function checksSummary(unit: LeadWorkUnitView): string {
  return `${unit.acceptanceCriteria.length} acceptance · ${unit.negativeCases.length} negative`;
}

export function Lead() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [actions, setActions] = useState<Action[]>([]);
  const [delegations, setDelegations] = useState<StandingDelegation[]>([]);
  const [requests, setRequests] = useState<LeadRequestRecord[]>([]);
  const [selected, setSelected] = useState<LeadRequestProjection | null>(null);
  const [projectId, setProjectId] = useState("");
  const [actionId, setActionId] = useState("");
  const [delegationId, setDelegationId] = useState("");
  const [objective, setObjective] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
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
    if (!delegationId && nextDelegations[0])
      setDelegationId(nextDelegations[0].id);
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
              : "Failed to load Lead data.",
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
  }, [load]);

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const project = projects.find((item) => item.id === projectId);
      if (!project) throw new Error("Select a Project.");
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
      setNotice(`Lead request ${shortId(request.id)} submitted.`);
      setSelected(await coreApi.getLeadRequest(request.id));
      await load();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Lead request failed.");
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
      setNotice("Deterministic fixture assessment recorded.");
      await load();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Assessment failed.");
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
      setNotice(
        "Plan frozen and marked ready; autonomous start remains disabled.",
      );
      await load();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Freeze failed.");
    } finally {
      setBusy(false);
    }
  }

  async function view(id: string) {
    setError(null);
    try {
      setSelected(await coreApi.getLeadRequest(id));
    } catch (cause) {
      setError(
        cause instanceof Error
          ? cause.message
          : "Failed to load the Lead request.",
      );
    }
  }

  const projectActions = actions.filter(
    (action) => action.project_id === projectId,
  );
  const projectDelegations = delegations.filter(
    (delegation) => delegation.projectId === projectId && !delegation.revokedAt,
  );
  const selectedAssessment = selected
    ? parseJson<LeadAssessmentView>(selected.assessment?.assessment_json)
    : null;
  const selectedProposal = selected
    ? parseJson<LeadPlanProposalView>(selected.proposal?.proposal_json)
    : null;

  return (
    <div>
      <h2>Lead</h2>
      <p className="muted">
        Lead is a projection of Core-owned state. The deterministic fixture
        assessor is available only when the Core is explicitly started with
        <code> FACTORY_LEAD_FIXTURE_ASSESSOR=1</code>; it is not a model and
        cannot authorize a Campaign.
      </p>
      {error && <p className="error">{error}</p>}
      {notice && <p className="notice">{notice}</p>}

      <form className="create" onSubmit={submit}>
        <h3>Submit a Lead request</h3>
        <label>
          Project
          <select
            data-testid="lead-project"
            value={projectId}
            onChange={(event) => setProjectId(event.target.value)}
          >
            <option value="">Select a Project</option>
            {projects.map((project) => (
              <option key={project.id} value={project.id}>
                {shortId(project.id)} — {project.repository}
              </option>
            ))}
          </select>
        </label>
        <label>
          Frozen Action
          <select
            data-testid="lead-action"
            value={actionId}
            onChange={(event) => setActionId(event.target.value)}
          >
            <option value="">Select an Action</option>
            {projectActions.map((action) => (
              <option key={action.id} value={action.id}>
                {shortId(action.id)} — {action.objective}
              </option>
            ))}
          </select>
        </label>
        <label>
          Active delegation
          <select
            data-testid="lead-delegation"
            value={delegationId}
            onChange={(event) => setDelegationId(event.target.value)}
          >
            <option value="">Select a delegation</option>
            {projectDelegations.map((delegation) => (
              <option key={delegation.id} value={delegation.id}>
                {delegation.id} — {delegation.route.provider}/
                {delegation.route.model}
              </option>
            ))}
          </select>
        </label>
        <label>
          Objective
          <textarea
            data-testid="lead-objective"
            value={objective}
            onChange={(event) => setObjective(event.target.value)}
            placeholder="Describe the intended outcome"
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
          {busy ? "Submitting…" : "Submit Lead request"}
        </button>
      </form>

      <h3>Lead requests ({requests.length})</h3>
      {requests.length === 0 ? (
        <p className="muted">No Lead requests.</p>
      ) : (
        <table>
          <thead>
            <tr>
              <th>ID</th>
              <th>State</th>
              <th>Objective</th>
              <th>Campaign</th>
              <th>Updated</th>
              <th>Actions</th>
            </tr>
          </thead>
          <tbody>
            {requests.map((request) => (
              <tr key={request.id}>
                <td>
                  <code>{shortId(request.id)}</code>
                </td>
                <td>{request.state}</td>
                <td>{request.objective}</td>
                <td>
                  {request.campaign_id ? (
                    <Link to={`/campaigns/${request.campaign_id}`}>
                      <code>{shortId(request.campaign_id)}</code>
                    </Link>
                  ) : (
                    <span className="muted">—</span>
                  )}
                </td>
                <td className="muted">{formatTimestamp(request.updated_at)}</td>
                <td>
                  <button type="button" onClick={() => void view(request.id)}>
                    View
                  </button>
                  {["SUBMITTED", "ASSESSING"].includes(request.state) && (
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => void assess(request.id)}
                    >
                      Assess (fixture)
                    </button>
                  )}
                  {request.state === "PLANNED" && (
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => void freeze(request.id)}
                    >
                      Freeze plan
                    </button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {selected && (
        <section data-testid="lead-selected">
          <h3>Selected request</h3>
          <div className="view-head">
            <p>
              State: <strong>{selected.request.state}</strong> · Created{" "}
              {formatTimestamp(selected.request.created_at)} · Updated{" "}
              {formatTimestamp(selected.request.updated_at)}
            </p>
            {selected.request.campaign_id && (
              <Link to={`/campaigns/${selected.request.campaign_id}`}>
                Open Campaign {shortId(selected.request.campaign_id)}
              </Link>
            )}
          </div>
          <dl className="detail">
            <dt>Request</dt>
            <dd>
              <code>{shortId(selected.request.id)}</code>
            </dd>
            <dt>Project</dt>
            <dd>
              <code>{shortId(selected.request.project_id)}</code>
            </dd>
            <dt>Action</dt>
            <dd>
              <code>{shortId(selected.request.action_id)}</code>
            </dd>
            <dt>Delegation</dt>
            <dd>
              <code>{selected.request.delegation_id}</code>
            </dd>
            <dt>Config hash</dt>
            <dd>
              <code title={selected.request.project_config_hash}>
                {shortId(selected.request.project_config_hash, 12)}
              </code>
            </dd>
          </dl>
          {selected.request.failure_reason && (
            <p className="error">{selected.request.failure_reason}</p>
          )}

          <h4>Assessment</h4>
          {selected.assessment ? (
            <>
              <p className="muted">
                Mechanism <strong>{selected.assessment.mechanism}</strong> ·
                recorded {formatTimestamp(selected.assessment.created_at)} ·
                schema{" "}
                <code>{shortId(selected.assessment.schema_sha256, 12)}</code>
              </p>
              {selectedAssessment ? (
                <dl className="detail" data-testid="lead-assessment">
                  <dt>Intent</dt>
                  <dd>{selectedAssessment.intent}</dd>
                  <dt>Complexity</dt>
                  <dd>{selectedAssessment.complexity}</dd>
                  <dt>Effort</dt>
                  <dd>
                    {selectedAssessment.effortEstimate.status === "estimated"
                      ? `${selectedAssessment.effortEstimate.minPersonHours}–${selectedAssessment.effortEstimate.maxPersonHours} ${selectedAssessment.effortEstimate.unit}`
                      : `${selectedAssessment.effortEstimate.status.replaceAll("_", " ")}${
                          selectedAssessment.effortEstimate.unknownReason
                            ? `: ${selectedAssessment.effortEstimate.unknownReason}`
                            : ""
                        }`}
                  </dd>
                  <dt>Evidence refs</dt>
                  <dd>{list(selectedAssessment.evidenceRefs)}</dd>
                  <dt>Change impact</dt>
                  <dd>{list(selectedAssessment.changeImpact)}</dd>
                  <dt>Validation needs</dt>
                  <dd>{list(selectedAssessment.validationNeeds)}</dd>
                  <dt>Knowledge gaps</dt>
                  <dd>{list(selectedAssessment.knowledgeGaps)}</dd>
                  <dt>Assumptions</dt>
                  <dd>{list(selectedAssessment.assumptions)}</dd>
                  <dt>Uncertainty</dt>
                  <dd>{list(selectedAssessment.uncertainty)}</dd>
                  <dt>Suggested next action</dt>
                  <dd>{selectedAssessment.suggestedNextAction}</dd>
                </dl>
              ) : (
                <p className="error">
                  Recorded assessment JSON could not be decoded.
                </p>
              )}
            </>
          ) : (
            <p className="muted">No assessment recorded.</p>
          )}

          <h4>Proposal</h4>
          {selected.proposal ? (
            <>
              <p className="muted">
                Proposal <code>{shortId(selected.proposal.id)}</code> · hash{" "}
                <code>{shortId(selected.proposal.proposal_hash, 12)}</code> ·
                recorded {formatTimestamp(selected.proposal.created_at)}
              </p>
              {selectedProposal ? (
                <div data-testid="lead-proposal">
                  <dl className="detail">
                    <dt>Class / risk</dt>
                    <dd>
                      {selectedProposal.class} / {selectedProposal.riskClass}
                    </dd>
                    <dt>Quality profile</dt>
                    <dd>{selectedProposal.qualityProfile}</dd>
                    <dt>Route</dt>
                    <dd>
                      {selectedProposal.route.provider}/
                      {selectedProposal.route.model} (
                      {selectedProposal.route.mode})
                    </dd>
                    <dt>Base revision</dt>
                    <dd>
                      <code title={selectedProposal.baseRevision}>
                        {shortId(selectedProposal.baseRevision, 12)}
                      </code>
                    </dd>
                    <dt>Budget</dt>
                    <dd>
                      {selectedProposal.budget.maxAttempts} attempt
                      {selectedProposal.budget.maxAttempts === 1 ? "" : "s"} ·{" "}
                      {selectedProposal.budget.timeoutSecs}s timeout ·{" "}
                      {selectedProposal.maxRepairCycles} repair cycle
                      {selectedProposal.maxRepairCycles === 1 ? "" : "s"} ·{" "}
                      {selectedProposal.maxSessions} session
                      {selectedProposal.maxSessions === 1 ? "" : "s"}
                    </dd>
                    <dt>Parallel / wall clock</dt>
                    <dd>
                      {selectedProposal.maxParallel} parallel ·{" "}
                      {selectedProposal.maxWallClockSecs}s wall clock
                    </dd>
                    <dt>Assumptions</dt>
                    <dd>{list(selectedProposal.assumptions)}</dd>
                    <dt>Risks</dt>
                    <dd>{list(selectedProposal.risks)}</dd>
                  </dl>
                  <h5>Work units ({selectedProposal.workUnits.length})</h5>
                  <table>
                    <thead>
                      <tr>
                        <th>#</th>
                        <th>Unit</th>
                        <th>Kind</th>
                        <th>Depends on</th>
                        <th>Paths</th>
                        <th>Checks</th>
                      </tr>
                    </thead>
                    <tbody>
                      {selectedProposal.workUnits.map((unit, index) => (
                        <tr key={unit.id}>
                          <td>{index + 1}</td>
                          <td>
                            <code>{unit.id}</code>
                            <div className="muted">{unit.description}</div>
                          </td>
                          <td>{unit.kind}</td>
                          <td>{unit.dependsOn.join(", ") || "none"}</td>
                          <td>
                            {unit.allowedPaths.join(", ") || "none"}
                            <div className="muted">
                              capabilities{" "}
                              {unit.capabilities.join(", ") || "none"}
                            </div>
                          </td>
                          <td>{checksSummary(unit)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              ) : (
                <p className="error">
                  Recorded proposal JSON could not be decoded.
                </p>
              )}
            </>
          ) : (
            <p className="muted">No proposal recorded.</p>
          )}

          <h4>Frozen plan</h4>
          {selected.plan ? (
            <dl className="detail" data-testid="lead-plan">
              <dt>Plan</dt>
              <dd>
                <code>{shortId(selected.plan.id)}</code>
              </dd>
              <dt>Plan hash</dt>
              <dd>
                <code title={selected.plan.plan_hash}>
                  {shortId(selected.plan.plan_hash, 12)}
                </code>
              </dd>
              <dt>Config hash</dt>
              <dd>
                <code title={selected.plan.config_hash}>
                  {shortId(selected.plan.config_hash, 12)}
                </code>
              </dd>
              <dt>Delegation hash</dt>
              <dd>
                <code title={selected.plan.delegation_hash}>
                  {shortId(selected.plan.delegation_hash, 12)}
                </code>
              </dd>
              <dt>Proposal</dt>
              <dd>
                <code>{shortId(selected.plan.proposal_id)}</code>
              </dd>
              <dt>Frozen</dt>
              <dd>{formatTimestamp(selected.plan.created_at)}</dd>
            </dl>
          ) : (
            <p className="muted">No frozen plan recorded.</p>
          )}

          {selected.assessment && (
            <details>
              <summary>Raw assessment JSON</summary>
              <pre>{selected.assessment.assessment_json}</pre>
            </details>
          )}
          {selected.proposal && (
            <details>
              <summary>Raw proposal JSON</summary>
              <pre>{selected.proposal.proposal_json}</pre>
            </details>
          )}
          <p className="muted">
            Autonomy, budget expansion, provider execution and delivery are not
            granted by this view.
          </p>
        </section>
      )}
    </div>
  );
}
