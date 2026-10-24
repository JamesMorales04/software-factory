import { useCallback, useEffect, useRef, useState } from "react";
import {
  type Action,
  CoreApiError,
  coreApi,
  type LeadRequestProjection,
  type LeadRequestRecord,
  newIdempotencyKey,
  type Project,
  type StandingDelegation,
} from "../coreApi";
import { formatTimestamp, shortId } from "../format";

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

  const projectActions = actions.filter(
    (action) => action.project_id === projectId,
  );
  const projectDelegations = delegations.filter(
    (delegation) => delegation.projectId === projectId && !delegation.revokedAt,
  );

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
                <td className="muted">{formatTimestamp(request.updated_at)}</td>
                <td>
                  <button
                    type="button"
                    onClick={() =>
                      void coreApi.getLeadRequest(request.id).then(setSelected)
                    }
                  >
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
        <section>
          <h3>Selected request</h3>
          <p>
            State: <strong>{selected.request.state}</strong> · Plan hash:{" "}
            <code>{selected.plan?.plan_hash ?? "not frozen"}</code>
          </p>
          {selected.request.failure_reason && (
            <p className="error">{selected.request.failure_reason}</p>
          )}
          <h4>Assessment</h4>
          <pre>
            {selected.assessment?.assessment_json ?? "No assessment recorded."}
          </pre>
          <h4>Proposal</h4>
          <pre>
            {selected.proposal?.proposal_json ?? "No proposal recorded."}
          </pre>
          <p className="muted">
            Autonomy, budget expansion, provider execution and delivery are not
            granted by this view.
          </p>
        </section>
      )}
    </div>
  );
}
