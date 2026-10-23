import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import {
  type Action,
  type Campaign,
  type Candidate,
  CoreApiError,
  coreApi,
  type FactoryEvent,
  type Health,
  type Project,
  type Run,
} from "../coreApi";
import { formatTimestamp, shortId } from "../format";

interface Snapshot {
  health: Health;
  projects: Project[];
  actions: Action[];
  campaigns: Campaign[];
  runs: Run[];
  candidates: Candidate[];
  events: FactoryEvent[];
}

function countBy<T>(items: T[], key: (item: T) => string) {
  const counts: Record<string, number> = { total: items.length };
  for (const item of items) {
    const value = key(item);
    counts[value] = (counts[value] ?? 0) + 1;
  }
  return counts;
}

function statusSummary(counts: Record<string, number>): string {
  const entries = Object.entries(counts).filter(
    ([status]) => status !== "total",
  );
  if (entries.length === 0) {
    return "none";
  }
  return entries
    .map(
      ([status, count]) =>
        `${count} ${status.toLowerCase().replaceAll("_", " ")}`,
    )
    .join(" · ");
}

export function Dashboard() {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    async function load() {
      try {
        const [health, projects, actions, campaigns, runs, candidates, events] =
          await Promise.all([
            coreApi.health(),
            coreApi.listProjects(),
            coreApi.listActions(),
            coreApi.listCampaigns(),
            coreApi.listRuns(),
            coreApi.listCandidates(),
            coreApi.listEvents(),
          ]);
        if (!active) {
          return;
        }
        setSnapshot({
          health,
          projects,
          actions,
          campaigns,
          runs,
          candidates,
          events,
        });
        setError(null);
      } catch (cause) {
        if (active) {
          setError(
            cause instanceof CoreApiError
              ? cause.message
              : "Failed to load dashboard.",
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

  if (error && !snapshot) {
    return <p className="error">{error}</p>;
  }
  if (!snapshot) {
    return <p className="muted">Loading…</p>;
  }

  const actionCounts = countBy(snapshot.actions, (action) => action.status);
  const campaignCounts = countBy(
    snapshot.campaigns,
    (campaign) => campaign.status,
  );
  const runCounts = countBy(snapshot.runs, (run) => run.status);
  const recentEvents = [...snapshot.events].reverse().slice(0, 20);

  return (
    <div>
      <h2>Dashboard</h2>
      <p className="muted">
        Authoritative Rust Core snapshots, refreshed every four seconds.
      </p>
      {error && <p className="error">{error}</p>}
      <section className="cards">
        <div className="card">
          <h3>Core</h3>
          <p>
            {snapshot.health.healthy ? "healthy" : "unhealthy"} · version{" "}
            <code>{snapshot.health.version}</code>
          </p>
        </div>
        <div className="card">
          <h3>Projects</h3>
          <p>{snapshot.projects.length} registered</p>
          <Link to="/projects">Open Projects</Link>
        </div>
        <div className="card">
          <h3>Actions</h3>
          <p>
            {snapshot.actions.length} total · {statusSummary(actionCounts)}
          </p>
          <Link to="/actions">Open Actions</Link>
        </div>
        <div className="card">
          <h3>Campaigns</h3>
          <p>
            {snapshot.campaigns.length} total · {statusSummary(campaignCounts)}
          </p>
          <Link to="/campaigns">Open Campaigns</Link>
        </div>
        <div className="card">
          <h3>Runs</h3>
          <p>
            {snapshot.runs.length} total · {statusSummary(runCounts)}
          </p>
          <Link to="/runs">Open Runs</Link>
        </div>
        <div className="card">
          <h3>Candidates</h3>
          <p>{snapshot.candidates.length} recorded</p>
          <Link to="/campaigns">Open quality gates</Link>
        </div>
        <div className="card">
          <h3>Event ledger</h3>
          <p>{snapshot.events.length} events recorded</p>
        </div>
      </section>

      <h3>Recent events ({recentEvents.length})</h3>
      {recentEvents.length === 0 ? (
        <p className="muted">
          No events recorded. Register a Project or create an Action to move the
          ledger.
        </p>
      ) : (
        <ul className="events">
          {recentEvents.map((event) => (
            <li key={event.sequence}>
              <div className="event-head">
                <code>#{event.sequence}</code> <strong>{event.kind}</strong>{" "}
                <span className="muted">
                  {shortId(event.subject_id)} ·{" "}
                  {formatTimestamp(event.recorded_at)}
                </span>
              </div>
              <p className="event-detail">{event.detail}</p>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
