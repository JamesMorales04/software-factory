import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useSearchParams } from "react-router-dom";
import {
  ApiError,
  api,
  type FactoryEvent,
  type LeadRequest,
  newIdempotencyKey,
  type Project,
} from "../api";

function projectionText(value: unknown): string {
  if (value === undefined || value === null || value === "")
    return "Not provided by Core.";
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean")
    return String(value);
  return JSON.stringify(value, null, 2);
}

function ProjectionField({ label, value }: { label: string; value: unknown }) {
  return (
    <section className="card">
      <h3>{label}</h3>
      {value === undefined || value === null || value === "" ? (
        <p className="muted">Not provided by Core.</p>
      ) : typeof value === "string" || typeof value === "number" ? (
        <p>{projectionText(value)}</p>
      ) : (
        <pre>{projectionText(value)}</pre>
      )}
    </section>
  );
}

function ProjectionFields({
  title,
  values,
}: {
  title: string;
  values: Array<[string, unknown]>;
}) {
  return (
    <section className="card">
      <h3>{title}</h3>
      <dl>
        {values.map(([label, value]) => (
          <div key={label}>
            <dt>
              <strong>{label}</strong>
            </dt>
            <dd>{projectionText(value)}</dd>
          </div>
        ))}
      </dl>
    </section>
  );
}

export function Lead() {
  const [searchParams, setSearchParams] = useSearchParams();
  const [projects, setProjects] = useState<Project[]>([]);
  const [projectId, setProjectId] = useState("");
  const [objective, setObjective] = useState("");
  const [delegationId, setDelegationId] = useState("");
  const [leadRequest, setLeadRequest] = useState<LeadRequest | null>(null);
  const [events, setEvents] = useState<FactoryEvent[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const attemptKey = useRef(newIdempotencyKey());

  const refreshProjects = useCallback(async () => {
    try {
      const registered = (await api.listProjects()).filter(
        (project) => project.registrationStatus === "registered",
      );
      setProjects(registered);
      setProjectId((current) => current || registered[0]?.id || "");
    } catch (e) {
      setError(e instanceof ApiError ? e.message : "Failed to load Projects.");
    }
  }, []);

  useEffect(() => {
    void refreshProjects();
  }, [refreshProjects]);

  useEffect(() => {
    const requestId = searchParams.get("request");
    if (!requestId) {
      setLeadRequest(null);
      setEvents([]);
      return;
    }
    let live = true;
    setError(null);
    void api
      .getLeadRequest(requestId)
      .then((request) => {
        if (live) setLeadRequest(request);
      })
      .catch((e: unknown) => {
        if (live) {
          setLeadRequest(null);
          setError(
            e instanceof ApiError ? e.message : "Failed to load Lead request.",
          );
        }
      });
    void api
      .getLeadRequestEvents(requestId)
      .then((current) => {
        if (live) setEvents(current);
      })
      .catch((e: unknown) => {
        if (live) {
          setError(
            e instanceof ApiError
              ? e.message
              : "Failed to load Lead event history.",
          );
        }
      });
    return () => {
      live = false;
    };
  }, [searchParams]);

  useEffect(() => {
    if (!leadRequest?.id) return;
    let live = true;
    const refresh = async () => {
      try {
        const current = await api.getLeadRequest(leadRequest.id);
        if (live) setLeadRequest(current);
        const currentEvents = await api.getLeadRequestEvents(leadRequest.id);
        if (live) setEvents(currentEvents);
      } catch (e) {
        if (live)
          setError(
            e instanceof ApiError
              ? e.message
              : "Failed to refresh Lead request.",
          );
      }
    };
    const timer = setInterval(() => void refresh(), 3000);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [leadRequest?.id]);

  async function onSubmit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const { data, replayed } = await api.createLeadRequest(
        {
          projectId,
          objective: objective.trim(),
          delegationId: delegationId.trim(),
        },
        attemptKey.current,
      );
      setLeadRequest(data.request);
      setSearchParams({ request: data.request.id });
      setNotice(
        replayed
          ? "This submission was already recorded."
          : "Lead request submitted.",
      );
      attemptKey.current = newIdempotencyKey();
      setObjective("");
      setDelegationId("");
    } catch (e) {
      setError(
        e instanceof ApiError ? e.message : "Lead request submission failed.",
      );
    } finally {
      setBusy(false);
    }
  }

  async function start() {
    if (!leadRequest) return;
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      setLeadRequest(await api.startLeadRequest(leadRequest.id));
      setNotice("Lead request start was recorded by Core.");
    } catch (e) {
      setError(
        e instanceof ApiError
          ? e.message
          : "Lead request could not be started.",
      );
    } finally {
      setBusy(false);
    }
  }

  const canStart =
    leadRequest?.status === "ready" &&
    leadRequest.authorizationState === "permitted";

  return (
    <div>
      <h2>Lead</h2>
      <p className="muted">
        Submit an objective for a server-owned assessment and plan. Core
        controls authorization and records every resulting state.
      </p>
      {error && <p className="error">{error}</p>}
      {notice && <p className="notice">{notice}</p>}
      <form className="create" onSubmit={onSubmit}>
        <h3>New Lead request</h3>
        <label>
          Registered Project
          <select
            data-testid="lead-project"
            value={projectId}
            onChange={(e) => setProjectId(e.target.value)}
            required
          >
            <option value="">Select a registered Project…</option>
            {projects.map((project) => (
              <option key={project.id} value={project.id}>
                {project.name} ({project.id})
              </option>
            ))}
          </select>
          {projects.length === 0 && (
            <span className="muted">No registered Projects are available.</span>
          )}
        </label>
        <label>
          Objective
          <textarea
            data-testid="lead-objective"
            value={objective}
            onChange={(e) => setObjective(e.target.value)}
            maxLength={4000}
            rows={4}
            placeholder="Describe the outcome you want."
            required
          />
        </label>
        <label>
          Delegation ID
          <input
            data-testid="lead-delegation"
            value={delegationId}
            onChange={(e) => setDelegationId(e.target.value)}
            maxLength={200}
            required
          />
        </label>
        <button
          type="submit"
          disabled={
            busy ||
            !projectId ||
            objective.trim() === "" ||
            delegationId.trim() === ""
          }
        >
          {busy ? "Submitting…" : "Submit objective"}
        </button>
      </form>

      {leadRequest && (
        <section aria-label="Lead request projection">
          <h3>
            Request <code>{leadRequest.id}</code>
          </h3>
          <div className="cards">
            <section className="card">
              <h3>Request state</h3>
              <p>
                <span className="pill">{leadRequest.status}</span>
              </p>
              <p>
                Project: <code>{leadRequest.projectId}</code>
              </p>
              <p>
                Delegation:{" "}
                <code>{leadRequest.delegationId || "Not provided"}</code>
              </p>
              <p>
                Authorization:{" "}
                <strong>
                  {leadRequest.authorizationState ?? "Not provided by Core"}
                </strong>
              </p>
              {leadRequest.authorizationState !== "permitted" && (
                <p className="muted">No authority to start is recorded.</p>
              )}
              {leadRequest.authorizationState === "permitted" && !canStart && (
                <p className="muted">
                  Core has not marked this request ready to start.
                </p>
              )}
              {canStart && (
                <button
                  type="button"
                  onClick={() => void start()}
                  disabled={busy}
                >
                  {busy ? "Starting…" : "Start authorized request"}
                </button>
              )}
            </section>
            <ProjectionFields
              title="Assessment"
              values={[
                ["Intent", leadRequest.assessment?.intent],
                ["Complexity", leadRequest.assessment?.complexity],
                ["Knowledge gaps", leadRequest.assessment?.knowledgeGaps],
                ["Change impact", leadRequest.assessment?.changeImpact],
                ["Validation needs", leadRequest.assessment?.validationNeeds],
                ["Assumptions", leadRequest.assessment?.assumptions],
                [
                  "Suggested next action",
                  leadRequest.assessment?.suggestedNextAction,
                ],
                [
                  "Effort estimate status",
                  leadRequest.assessment?.effortEstimate.status,
                ],
                [
                  "Effort estimate unit",
                  leadRequest.assessment?.effortEstimate.unit,
                ],
                [
                  "Person-hour range",
                  {
                    min: leadRequest.assessment?.effortEstimate.minPersonHours,
                    max: leadRequest.assessment?.effortEstimate.maxPersonHours,
                  },
                ],
                [
                  "Effort estimate assumptions",
                  leadRequest.assessment?.effortEstimate.assumptions,
                ],
                [
                  "Unknown effort reason",
                  leadRequest.assessment?.effortEstimate.unknownReason,
                ],
                ["Evidence references", leadRequest.assessment?.evidenceRefs],
                ["Assessment schema", leadRequest.assessment?.schemaRevision],
              ]}
            />
            <ProjectionFields
              title="Plan"
              values={[
                ["Objective", leadRequest.plan?.objective],
                ["Objective class", leadRequest.plan?.class],
                ["Authorized base revision", leadRequest.plan?.baseRevision],
                [
                  "Project configuration hash",
                  leadRequest.plan?.projectConfigHash,
                ],
                ["Quality profile", leadRequest.plan?.qualityProfile],
                [
                  "Model route",
                  {
                    provider: leadRequest.plan?.provider,
                    model: leadRequest.plan?.model,
                    paseoMode: leadRequest.plan?.paseoMode,
                  },
                ],
                ["Work units", leadRequest.plan?.workUnits],
                ["Assumptions", leadRequest.plan?.assumptions],
                ["Risk statements", leadRequest.plan?.risks],
                [
                  "Accepted paths",
                  leadRequest.plan?.workUnits?.flatMap(
                    (unit) => unit.allowedPaths ?? [],
                  ),
                ],
                ["Acceptance checks", leadRequest.plan?.acceptanceCriteria],
                ["Negative checks", leadRequest.plan?.negativeCases],
                ["Risk class", leadRequest.plan?.riskClass],
                ["Budget", leadRequest.plan?.budget],
                [
                  "Campaign limits",
                  {
                    maxRepairCycles: leadRequest.plan?.maxRepairCycles,
                    maxWallClockSecs: leadRequest.plan?.maxWallClockSecs,
                    maxParallel: leadRequest.plan?.maxParallel,
                    maxSessions: leadRequest.plan?.maxSessions,
                  },
                ],
                ["Plan hash", leadRequest.planHash],
              ]}
            />
            <ProjectionField
              label="Uncertainty"
              value={leadRequest.assessment?.uncertainty}
            />
            <ProjectionFields
              title="Model observation"
              values={[
                ["Decision mechanism", leadRequest.modelObservation?.mechanism],
                ["Provider", leadRequest.modelObservation?.provider],
                ["Model", leadRequest.modelObservation?.model],
                ["Mode", leadRequest.modelObservation?.mode],
                ["Session ID", leadRequest.modelObservation?.sessionId],
                ["Elapsed (ns)", leadRequest.modelObservation?.elapsed],
                ["Input bytes", leadRequest.modelObservation?.inputBytes],
                ["Output bytes", leadRequest.modelObservation?.outputBytes],
              ]}
            />
            {leadRequest.failureReason && (
              <p className="error">Core reason: {leadRequest.failureReason}</p>
            )}
          </div>
          <section className="card">
            <h3>Linked records</h3>
            <ul>
              <li>
                Action:{" "}
                {leadRequest.actionId ? (
                  <Link
                    to={`/actions/${encodeURIComponent(leadRequest.actionId)}`}
                  >
                    <code>{leadRequest.actionId}</code>
                  </Link>
                ) : (
                  <span className="muted">Not created</span>
                )}
              </li>
              <li>
                ActionSpec:{" "}
                {leadRequest.actionSpecId ? (
                  leadRequest.actionId ? (
                    <Link
                      to={`/actions/${encodeURIComponent(leadRequest.actionId)}`}
                    >
                      <code>{leadRequest.actionSpecId}</code>
                    </Link>
                  ) : (
                    <code>{leadRequest.actionSpecId}</code>
                  )
                ) : (
                  <span className="muted">Not frozen</span>
                )}
              </li>
              <li>
                Campaign:{" "}
                {leadRequest.campaignId ? (
                  <Link
                    to={`/campaigns/${encodeURIComponent(leadRequest.campaignId)}`}
                  >
                    <code>{leadRequest.campaignId}</code>
                  </Link>
                ) : (
                  <span className="muted">Not created</span>
                )}
              </li>
              <li>
                Campaign state:{" "}
                {leadRequest.campaignStatus ?? (
                  <span className="muted">Not created</span>
                )}
              </li>
            </ul>
          </section>
          <section className="card" aria-label="Lead event history">
            <h3>Core event history</h3>
            {events.length === 0 ? (
              <p className="muted">No recorded events.</p>
            ) : (
              <ol>
                {events.map((event) => (
                  <li key={event.eventId}>
                    <strong>{event.eventType}</strong> · {event.occurredAt}
                  </li>
                ))}
              </ol>
            )}
          </section>
        </section>
      )}
    </div>
  );
}
