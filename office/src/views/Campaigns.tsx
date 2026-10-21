import { useCallback, useEffect, useMemo, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import {
  type Action,
  type ActionSpec,
  ApiError,
  api,
  type Campaign,
  newIdempotencyKey,
  type Project,
} from "../api";

export function Campaigns() {
  const navigate = useNavigate();
  const [campaigns, setCampaigns] = useState<Campaign[]>([]);
  const [actions, setActions] = useState<Action[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [specs, setSpecs] = useState<ActionSpec[]>([]);
  const [projectFilter, setProjectFilter] = useState("");
  const [actionId, setActionId] = useState("");
  const [specId, setSpecId] = useState("");
  const [repairs, setRepairs] = useState(1);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [nextCampaigns, nextActions, nextProjects] = await Promise.all([
        api.listCampaigns(projectFilter ? { projectId: projectFilter } : {}),
        api.listActions(projectFilter ? { projectId: projectFilter } : {}),
        api.listProjects(),
      ]);
      setCampaigns(nextCampaigns);
      setActions(nextActions);
      setProjects(nextProjects);
    } catch (e) {
      setError(e instanceof ApiError ? e.message : "Failed to load campaigns.");
    }
  }, [projectFilter]);

  useEffect(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), 3000);
    return () => clearInterval(timer);
  }, [refresh]);

  useEffect(() => {
    if (!actionId) {
      setSpecs([]);
      setSpecId("");
      return;
    }
    let live = true;
    api
      .listActionSpecs(actionId)
      .then((next) => {
        if (live) {
          setSpecs(next);
          setSpecId(next[0]?.id ?? "");
        }
      })
      .catch(() => {
        if (live) {
          setSpecs([]);
          setSpecId("");
        }
      });
    return () => {
      live = false;
    };
  }, [actionId]);

  const canCreate = useMemo(
    () => actionId !== "" && specId !== "",
    [actionId, specId],
  );

  async function onCreate(e: React.FormEvent) {
    e.preventDefault();
    if (!canCreate) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const { data: created } = await api.createCampaign(
        {
          specId,
          maxRepairCycles: repairs,
          maxWallClockSecs: 1800,
          maxParallel: 2,
        },
        newIdempotencyKey(),
      );
      await api.startCampaign(created.campaign.id);
      navigate(`/campaigns/${created.campaign.id}`);
    } catch (err) {
      setError(
        err instanceof ApiError ? err.message : "Campaign creation failed.",
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <div>
      <h2>Campaigns</h2>
      {error && <p className="error">{error}</p>}
      <form className="create" onSubmit={onCreate}>
        <h3>Start an engineering campaign</h3>
        <label>
          Project filter
          <select
            data-testid="campaign-project-filter"
            value={projectFilter}
            onChange={(e) => {
              setProjectFilter(e.target.value);
              setActionId("");
            }}
          >
            <option value="">All Projects</option>
            {projects
              .filter((project) => project.registrationStatus === "registered")
              .map((project) => (
                <option key={project.id} value={project.id}>
                  {project.name} ({project.id})
                </option>
              ))}
          </select>
        </label>
        <label>
          Action
          <select
            data-testid="campaign-action"
            value={actionId}
            onChange={(e) => setActionId(e.target.value)}
            required
          >
            <option value="">Select an Action…</option>
            {actions.map((action) => (
              <option key={action.id} value={action.id}>
                {action.title} ({action.status})
              </option>
            ))}
          </select>
        </label>
        <label>
          Frozen ActionSpec
          <select
            data-testid="campaign-spec"
            value={specId}
            onChange={(e) => setSpecId(e.target.value)}
            required
            disabled={specs.length === 0}
          >
            {specs.length === 0 && <option value="">No frozen spec</option>}
            {specs.map((spec) => (
              <option key={spec.id} value={spec.id}>
                {spec.id.slice(0, 8)} · {spec.riskClass} · {spec.policyRevision}{" "}
                · {spec.qualityProfile}
              </option>
            ))}
          </select>
          {actionId && specs.length === 0 && (
            <span className="muted">This Action has no frozen ActionSpec.</span>
          )}
        </label>
        <label>
          Repair budget (0–3)
          <input
            type="number"
            min={0}
            max={3}
            value={repairs}
            onChange={(e) => setRepairs(Number(e.target.value))}
          />
        </label>
        <button type="submit" disabled={busy || !canCreate}>
          {busy ? "Starting…" : "Create & start campaign"}
        </button>
      </form>

      <h3>All campaigns</h3>
      {campaigns.length === 0 ? (
        <p className="muted">No campaigns yet.</p>
      ) : (
        <table>
          <thead>
            <tr>
              <th>Campaign</th>
              <th>Project</th>
              <th>Action</th>
              <th>Status</th>
              <th>Repairs</th>
              <th>Updated</th>
            </tr>
          </thead>
          <tbody>
            {campaigns.map((campaign) => (
              <tr key={campaign.id}>
                <td>
                  <Link to={`/campaigns/${campaign.id}`}>
                    <code>{campaign.id.slice(0, 8)}</code>
                  </Link>
                </td>
                <td>
                  <code>{campaign.projectId}</code>
                </td>
                <td>
                  <Link to={`/actions/${campaign.actionId}`}>
                    <code>{campaign.actionId.slice(0, 8)}</code>
                  </Link>
                </td>
                <td>
                  <span className={`pill campaign-${campaign.status}`}>
                    {campaign.status}
                  </span>
                </td>
                <td>
                  {campaign.repairCycles}/{campaign.maxRepairCycles}
                </td>
                <td className="muted">
                  {new Date(campaign.updatedAt).toLocaleString()}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
