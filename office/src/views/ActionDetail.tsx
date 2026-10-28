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
import { shortId } from "../format";
import { useI18n } from "../i18n";

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
  const { t, formatDate, formatNumber } = useI18n();
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
              : t("actionDetail.error.load"),
          );
        }
      }
    }
    void load();
    return () => {
      active = false;
    };
  }, [actionId, t]);

  if (error && !loaded) {
    return (
      <ErrorState
        title={t("common.errorTitle")}
        message={error}
        action={
          <Button onClick={() => window.location.reload()}>
            {t("common.retry")}
          </Button>
        }
      />
    );
  }
  if (missing) {
    return (
      <EmptyState
        title={t("actionDetail.notFoundTitle")}
        description={t("actionDetail.notFoundDescription", { id: actionId })}
        action={<Link to="/actions">{t("actionDetail.back")}</Link>}
      />
    );
  }
  if (!loaded) {
    return <LoadingState label={t("actionDetail.loading")} />;
  }

  const { action, project, campaigns, leadRequests, delegations } = loaded;

  return (
    <div>
      <PageHeader
        title={t("actionDetail.title", { id: shortId(action.id) })}
        description={action.objective}
        meta={
          <>
            <Badge tone={statusTone(action.status)}>{action.status}</Badge>
            <span>{t("actionDetail.risk", { risk: action.risk })}</span>
            <span>{formatDate(action.created_at)}</span>
          </>
        }
        actions={
          <Link
            className="ui-button ui-button--quiet ui-button--sm"
            to="/actions"
          >
            {t("actionDetail.back")}
          </Link>
        }
      />
      {error && <p className="error">{error}</p>}

      <div className="overview-grid">
        <Panel>
          <div className="section-heading">
            <h2>{t("actionDetail.specTitle")}</h2>
            <code title={action.spec_sha256}>
              {t("actionDetail.specHash", {
                id: shortId(action.spec_sha256, 12),
              })}
            </code>
          </div>
          <dl className="overview-facts">
            <div>
              <dt>{t("actionDetail.project")}</dt>
              <dd>
                {project ? (
                  <Link to="/projects">{shortId(project.id)}</Link>
                ) : (
                  shortId(action.project_id)
                )}
              </dd>
            </div>
            <div>
              <dt>{t("actionDetail.route")}</dt>
              <dd>
                {action.provider}/{action.model}
              </dd>
            </div>
            <div>
              <dt>{t("actionDetail.baseCommit")}</dt>
              <dd>
                <code>{shortId(action.spec.base_commit, 12)}</code>
              </dd>
            </div>
            <div>
              <dt>{t("actionDetail.riskClass")}</dt>
              <dd>{action.risk}</dd>
            </div>
          </dl>
          <p className="muted">{t("actionDetail.allowedPaths")}</p>
          <p>
            {action.spec.allowed_paths.length === 0
              ? t("request.values.noneRecorded")
              : action.spec.allowed_paths.join(", ")}
          </p>
        </Panel>

        <Panel>
          <div className="section-heading">
            <h2>{t("actionDetail.relationships")}</h2>
            <span className="muted">{t("actionDetail.relationshipsHint")}</span>
          </div>
          <dl className="overview-facts">
            <div>
              <dt>{t("actionDetail.campaigns")}</dt>
              <dd>{formatNumber(campaigns.length)}</dd>
            </div>
            <div>
              <dt>{t("actionDetail.leadRequests")}</dt>
              <dd>{formatNumber(leadRequests.length)}</dd>
            </div>
            <div>
              <dt>{t("actionDetail.delegations")}</dt>
              <dd>{formatNumber(delegations.length)}</dd>
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
                      ? t("actionDetail.campaignLinked", {
                          id: shortId(request.campaign_id),
                        })
                      : t("actionDetail.noCampaign")}
                  </p>
                </li>
              ))}
            </ul>
          )}
          {delegations.length > 0 && (
            <p className="muted">
              {t("actionDetail.delegationsList")}{" "}
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
          <h2>
            {t("actionDetail.checks", {
              count: formatNumber(action.spec.checks.length),
            })}
          </h2>
          <span className="muted">{t("actionDetail.checksHint")}</span>
        </div>
        {action.spec.checks.length === 0 ? (
          <EmptyState
            title={t("actionDetail.noChecksTitle")}
            description={t("actionDetail.noChecksDescription")}
          />
        ) : (
          <div className="table-responsive">
            <table aria-label={t("actionDetail.table.label")}>
              <thead>
                <tr>
                  <th>{t("actionDetail.table.id")}</th>
                  <th>{t("actionDetail.table.kind")}</th>
                  <th>{t("actionDetail.table.command")}</th>
                  <th>{t("actionDetail.table.expect")}</th>
                  <th>{t("actionDetail.table.timeout")}</th>
                </tr>
              </thead>
              <tbody>
                {action.spec.checks.map((check) => (
                  <tr key={check.id}>
                    <td data-label={t("actionDetail.table.id")}>
                      <code>{check.id}</code>
                    </td>
                    <td data-label={t("actionDetail.table.kind")}>
                      {check.kind}
                    </td>
                    <td data-label={t("actionDetail.table.command")}>
                      <code>{check.command.join(" ")}</code>
                      <div className="muted">{check.description}</div>
                    </td>
                    <td data-label={t("actionDetail.table.expect")}>
                      {check.expectExit}
                    </td>
                    <td data-label={t("actionDetail.table.timeout")}>
                      {t("actionDetail.timeoutValue", {
                        seconds: check.timeoutSecs,
                      })}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Panel>
    </div>
  );
}
