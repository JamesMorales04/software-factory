import { useCallback, useEffect, useState } from "react";
import {
  authorizeFixtureCampaign,
  createAction,
  createAttempt,
  createCampaign,
  createRun,
  getGate,
  listActions,
  listCampaigns,
  listCandidates,
  listProjects,
  type NativeAction,
  NativeApiError,
  type NativeCampaign,
  type NativeCandidate,
  type NativeGate,
  type NativeProject,
  registerProject,
} from "../nativeApi";

export function Native() {
  const [projects, setProjects] = useState<NativeProject[]>([]);
  const [actions, setActions] = useState<NativeAction[]>([]);
  const [campaigns, setCampaigns] = useState<NativeCampaign[]>([]);
  const [candidates, setCandidates] = useState<NativeCandidate[]>([]);
  const [gates, setGates] = useState<Record<string, NativeGate | null>>({});
  const [repository, setRepository] = useState("");
  const [projectId, setProjectId] = useState("");
  const [objective, setObjective] = useState("fixture dispatch from Office");
  const [actionId, setActionId] = useState("");
  const [campaignId, setCampaignId] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const [nextProjects, nextActions, nextCampaigns, nextCandidates] =
        await Promise.all([
          listProjects(),
          listActions(),
          listCampaigns(),
          listCandidates(),
        ]);
      setProjects(nextProjects);
      setActions(nextActions);
      setCampaigns(nextCampaigns);
      setCandidates(nextCandidates);
      const nextGates: Record<string, NativeGate | null> = {};
      for (const candidate of nextCandidates) {
        nextGates[candidate.id] = await getGate(candidate.id);
      }
      setGates(nextGates);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Refresh failed.");
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), 4000);
    return () => clearInterval(timer);
  }, [refresh]);

  async function run<T>(action: () => Promise<T>, message: string) {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await action();
      setNotice(message);
      await refresh();
    } catch (cause) {
      setError(
        cause instanceof NativeApiError ? cause.message : "Action failed.",
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <section>
      <h2>Native Core</h2>
      <p className="muted">
        Deterministic Project/Action/Campaign path served by the Rust Core. This
        view uses operator credentials and does not start a paid provider.
      </p>
      {error && <p className="error">{error}</p>}
      {notice && <p className="notice">{notice}</p>}

      <h3>Register Project</h3>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          void run(
            () => registerProject(repository, `office-project-${Date.now()}`),
            "Project registered.",
          );
        }}
      >
        <input
          value={repository}
          onChange={(event) => setRepository(event.target.value)}
          placeholder="/path/to/git/repository"
        />
        <button type="submit" disabled={busy || repository.trim() === ""}>
          Register
        </button>
      </form>

      <h3>Create Action</h3>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          void run(
            () =>
              createAction({
                projectId,
                objective,
                provider: "opencode",
                model: "test/model",
                key: `office-action-${Date.now()}`,
                allowedPaths: ["src"],
              }),
            "Action created.",
          );
        }}
      >
        <select
          value={projectId}
          onChange={(event) => setProjectId(event.target.value)}
        >
          <option value="">Select Project</option>
          {projects.map((project) => (
            <option key={project.id} value={project.id}>
              {project.repository}
            </option>
          ))}
        </select>
        <input
          value={objective}
          onChange={(event) => setObjective(event.target.value)}
        />
        <button type="submit" disabled={busy || projectId === ""}>
          Create Action
        </button>
      </form>

      <h3>Campaign</h3>
      <div className="row">
        <select
          value={actionId}
          onChange={(event) => setActionId(event.target.value)}
        >
          <option value="">Select Action</option>
          {actions.map((action) => (
            <option key={action.id} value={action.id}>
              {action.objective} ({action.status})
            </option>
          ))}
        </select>
        <button
          type="button"
          disabled={busy || actionId === ""}
          onClick={() =>
            void run(
              () => createCampaign(actionId, `office-campaign-${Date.now()}`),
              "Campaign created.",
            )
          }
        >
          Create Campaign
        </button>
      </div>
      <div className="row">
        <select
          value={campaignId}
          onChange={(event) => setCampaignId(event.target.value)}
        >
          <option value="">Select Campaign</option>
          {campaigns.map((campaign) => (
            <option key={campaign.id} value={campaign.id}>
              {campaign.id} ({campaign.status})
            </option>
          ))}
        </select>
        <button
          type="button"
          disabled={busy || campaignId === ""}
          onClick={() =>
            void run(async () => {
              await authorizeFixtureCampaign(campaignId);
              const run = await createRun(campaignId);
              await createAttempt(run.id);
            }, "Campaign authorized and Attempt queued; start a fixture Worker to execute.")
          }
        >
          Authorize + Queue Attempt
        </button>
      </div>

      <h3>Candidates</h3>
      {candidates.length === 0 ? (
        <p className="muted">No candidates yet.</p>
      ) : (
        <table>
          <thead>
            <tr>
              <th>Candidate</th>
              <th>Commit</th>
              <th>Implementer</th>
              <th>Gate</th>
            </tr>
          </thead>
          <tbody>
            {candidates.map((candidate) => (
              <tr key={candidate.id}>
                <td>{candidate.id.slice(0, 8)}</td>
                <td>
                  <code>{candidate.commit_sha.slice(0, 12)}</code>
                </td>
                <td>{candidate.implementer_id}</td>
                <td>{gates[candidate.id]?.outcome ?? "PENDING"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}
