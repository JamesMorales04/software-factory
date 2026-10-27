import { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { Badge } from "../components/ui/Badge";
import { Button } from "../components/ui/Button";
import {
  EmptyState,
  ErrorState,
  LoadingState,
  PageHeader,
  Panel,
} from "../components/ui/Surface";
import {
  type Action,
  type Campaign,
  CoreApiError,
  coreApi,
  type LeadRequestRecord,
  type Project,
  type StandingDelegation,
} from "../coreApi";
import { formatTimestamp, shortId } from "../format";

interface LoadedAction {
  action: Action;
  project: Project | null;
  campaigns: Campaign[];
  leadRequests: LeadRequestRecord[];
  delegations: StandingDelegation[];
}

function statusTone(status: string) {
  if (status === "AUTHORIZED" || status === "READY") return "success" as const;
  if (status === "BLOCKED_AUTHORIZATION" || status === "HUMAN_REQUIRED") {
    return "attention" as const;
  }
  if (status === "FAILED" || status === "CANCELLED") return "danger" as const;
  return "info" as const;
}

export function ActionDetail() {
  const { actionId = "" } = useParams();
  const [loaded, setLoaded] = useState<LoadedAction | null>(null);
  const [missing, setMissing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    async function load() {
      try {
        const [actions, projects, campaigns, leadRequests, delegations] =
          await Promise.all([
            coreApi.listActions(),
            coreApi.listProjects(),
            coreApi.listCampaigns(),
            coreApi.listLeadRequests(),
            coreApi.listDelegations(),
          ]);
        if (!active) return;
        const action = actions.find((item) => item.id === actionId);
        if (!action) {
          setMissing(true);
          setLoaded(null);
          return;
        }
        setMissing(false);
        setLoaded({
          action,
          project:
            projects.find((item) => item.id === action.project_id) ?? null,
          campaigns: campaigns.filter((item) => item.action_id === action.id),
          leadRequests: leadRequests.filter(
            (item) => item.action_id === action.id,
          ),
          delegations: delegations.filter(
            (item) => item.projectId === action.project_id,
          ),
        });
        setError(null);
      } catch (cause) {
        if (active) {
          setError(
            cause instanceof CoreApiError
              ? cause.message
              : "Failed to load the Action.",
          );
        }
      }
    }
    void load();
    return () => {
      active = false;
    };
  }, [actionId]);

  if (error && !loaded) {
    return (
      <ErrorState
        message={error}
        action={<Button onClick={() => window.location.reload()}>Retry</Button>}
      />
    );
  }
  if (missing) {
    return (
      <EmptyState
        title="Action not found"
        description={`No Action ${actionId} is recorded in Core.`}
        action={<Link to="/actions">Back to Actions</Link>}
      />
    );
  }
  if (!loaded) {
    return <LoadingState label="Loading Action…" />;
  }

  const { action, project, campaigns, leadRequests, delegations } = loaded;

  return (
    <div>
      <PageHeader
        title={`Action ${shortId(action.id)}`}
        description={action.objective}
        meta={
          <>
            <Badge tone={statusTone(action.status)}>{action.status}</Badge>
            <span>Risk {action.risk}</span>
            <span>{formatTimestamp(action.created_at)}</span>
          </>
        }
        actions={
          <Link
            className="ui-button ui-button--quiet ui-button--sm"
            to="/actions"
          >
            Back to Actions
          </Link>
        }
      />
      {error && <p className="error">{error}</p>}

      <div className="overview-grid">
        <Panel>
          <div className="section-heading">
            <h2>Frozen specification</h2>
            <code title={action.spec_sha256}>
              spec {shortId(action.spec_sha256, 12)}
            </code>
          </div>
          <dl className="overview-facts">
            <div>
              <dt>Project</dt>
              <dd>
                {project ? (
                  <Link to="/projects">{shortId(project.id)}</Link>
                ) : (
                  shortId(action.project_id)
                )}
              </dd>
            </div>
            <div>
              <dt>Route</dt>
              <dd>
                {action.provider}/{action.model}
              </dd>
            </div>
            <div>
              <dt>Base commit</dt>
              <dd>
                <code>{shortId(action.spec.base_commit, 12)}</code>
              </dd>
            </div>
            <div>
              <dt>Risk class</dt>
              <dd>{action.risk}</dd>
            </div>
          </dl>
          <p className="muted">Allowed paths</p>
          <p>
            {action.spec.allowed_paths.length === 0
              ? "None recorded"
              : action.spec.allowed_paths.join(", ")}
          </p>
        </Panel>

        <Panel>
          <div className="section-heading">
            <h2>Relationships</h2>
            <span className="muted">Read-only Core projections</span>
          </div>
          <dl className="overview-facts">
            <div>
              <dt>Campaigns</dt>
              <dd>{campaigns.length}</dd>
            </div>
            <div>
              <dt>Lead requests</dt>
              <dd>{leadRequests.length}</dd>
            </div>
            <div>
              <dt>Delegations</dt>
              <dd>{delegations.length}</dd>
            </div>
          </dl>
          <div className="overview-badges">
            {campaigns.map((campaign) => (
              <Link key={campaign.id} to={`/campaigns/${campaign.id}`}>
                <Badge tone={statusTone(campaign.status)}>
                  {shortId(campaign.id)}: {campaign.status}
                </Badge>
              </Link>
            ))}
          </div>
          {leadRequests.length > 0 && (
            <ul className="overview-decisions">
              {leadRequests.map((request) => (
                <li key={request.id}>
                  <Link to="/lead">{shortId(request.id)}</Link>
                  <Badge tone={statusTone(request.state)}>
                    {request.state}
                  </Badge>
                  <p className="muted">
                    {request.campaign_id
                      ? `Campaign ${shortId(request.campaign_id)}`
                      : "No Campaign linked"}
                  </p>
                </li>
              ))}
            </ul>
          )}
          {delegations.length > 0 && (
            <p className="muted">
              Delegations:{" "}
              {delegations.map((delegation) => (
                <Link key={delegation.id} to="/delegations">
                  {shortId(delegation.id)}{" "}
                </Link>
              ))}
            </p>
          )}
        </Panel>
      </div>

      <Panel>
        <div className="section-heading">
          <h2>Frozen checks ({action.spec.checks.length})</h2>
          <span className="muted">Bound to every candidate</span>
        </div>
        {action.spec.checks.length === 0 ? (
          <EmptyState
            title="No checks"
            description="This Action cannot produce an accepted candidate without independent checks."
          />
        ) : (
          <table>
            <thead>
              <tr>
                <th>ID</th>
                <th>Kind</th>
                <th>Command</th>
                <th>Expect</th>
                <th>Timeout</th>
              </tr>
            </thead>
            <tbody>
              {action.spec.checks.map((check) => (
                <tr key={check.id}>
                  <td>
                    <code>{check.id}</code>
                  </td>
                  <td>{check.kind}</td>
                  <td>
                    <code>{check.command.join(" ")}</code>
                    <div className="muted">{check.description}</div>
                  </td>
                  <td>{check.expectExit}</td>
                  <td>{check.timeoutSecs}s</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Panel>
    </div>
  );
}
