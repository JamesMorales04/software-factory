import { useCallback, useEffect, useMemo, useState } from "react";
import { Link, useSearchParams } from "react-router-dom";
import { CancelCampaign } from "../CancelCampaign";
import { Badge } from "../components/ui/Badge";
import { Button } from "../components/ui/Button";
import { Select } from "../components/ui/Field";
import {
  EmptyState,
  ErrorState,
  LoadingState,
  PageHeader,
  Panel,
} from "../components/ui/Surface";
import { type Campaign, CoreApiError, coreApi, type Project } from "../coreApi";
import { formatTimestamp } from "../format";
import { useI18n } from "../i18n";
import { projectDisplayName } from "../workspace";
import "./campaigns.css";

const ACTIVE = new Set(["BLOCKED_AUTHORIZATION", "AUTHORIZED"]);

function stateTone(
  status: string,
): "success" | "attention" | "danger" | "neutral" {
  if (status === "AUTHORIZED") return "success";
  if (status === "BLOCKED_AUTHORIZATION") return "attention";
  if (status === "FAILED") return "danger";
  return "neutral";
}

function stateLabelKey(status: string) {
  switch (status) {
    case "BLOCKED_AUTHORIZATION":
      return "campaigns.state.blockedAuthorization" as const;
    case "AUTHORIZED":
      return "campaigns.state.authorized" as const;
    case "CANCELLED":
      return "campaigns.state.cancelled" as const;
    case "FAILED":
      return "campaigns.state.failed" as const;
    case "ACCEPTED":
      return "campaigns.state.accepted" as const;
    case "REPAIR_REQUIRED":
      return "campaigns.state.repairRequired" as const;
    default:
      return "campaigns.state.unknown" as const;
  }
}

export function Campaigns() {
  const { t, formatDate } = useI18n();
  const [searchParams, setSearchParams] = useSearchParams();
  const projectId = searchParams.get("project") ?? "";
  const [campaigns, setCampaigns] = useState<Campaign[] | null>(null);
  const [projects, setProjects] = useState<Project[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [lastLoadedAt, setLastLoadedAt] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const [nextCampaigns, nextProjects] = await Promise.all([
        coreApi.listCampaigns(),
        coreApi.listProjects(),
      ]);
      setCampaigns(nextCampaigns);
      setProjects(nextProjects);
      setError(null);
      setLastLoadedAt(new Date().toISOString());
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : t("campaigns.error.load"),
      );
    }
  }, [t]);

  useEffect(() => {
    void load();
    const timer = window.setInterval(() => void load(), 6000);
    return () => window.clearInterval(timer);
  }, [load]);

  const projectById = useMemo(
    () => new Map(projects.map((project) => [project.id, project])),
    [projects],
  );

  const rows = (campaigns ?? [])
    .map((campaign) => ({
      campaign,
      project: campaign.project_id
        ? projectById.get(campaign.project_id)
        : undefined,
    }))
    .filter((row) => !projectId || row.project?.id === projectId)
    .sort((left, right) => {
      const rightStatus =
        right.campaign.effective_status ?? right.campaign.status;
      const leftStatus = left.campaign.effective_status ?? left.campaign.status;
      const activeDelta =
        Number(ACTIVE.has(rightStatus)) - Number(ACTIVE.has(leftStatus));
      return (
        activeDelta ||
        right.campaign.created_at.localeCompare(left.campaign.created_at)
      );
    });

  function setProject(value: string) {
    setSearchParams(
      (current) => {
        const next = new URLSearchParams(current);
        if (value) next.set("project", value);
        else next.delete("project");
        return next;
      },
      { replace: true },
    );
  }

  return (
    <div className="campaigns-workspace">
      <PageHeader
        title={t("campaigns.title")}
        description={t("campaigns.descriptionSimplified")}
        meta={
          lastLoadedAt && (
            <span>
              {t("campaigns.updated", { date: formatTimestamp(lastLoadedAt) })}
            </span>
          )
        }
        actions={
          <Link className="ui-button ui-button--primary ui-button--md" to="/">
            {t("campaigns.newRequest")}
          </Link>
        }
      />

      <Panel className="campaigns-filter">
        <Select
          label={t("campaigns.filter.project")}
          value={projectId}
          onChange={(event) => setProject(event.target.value)}
        >
          <option value="">{t("campaigns.filter.allProjects")}</option>
          {projects.map((project) => (
            <option key={project.id} value={project.id}>
              {projectDisplayName(project)}
            </option>
          ))}
        </Select>
      </Panel>

      {error && (
        <ErrorState
          message={error}
          action={
            <Button onClick={() => void load()}>{t("common.retry")}</Button>
          }
        />
      )}
      {!error && campaigns === null && (
        <LoadingState label={t("campaigns.loading")} />
      )}
      {!error && campaigns !== null && rows.length === 0 && (
        <Panel>
          <EmptyState
            title={t("campaigns.emptyTitle")}
            description={t("campaigns.emptyDescription")}
            action={
              <Link
                className="ui-button ui-button--primary ui-button--md"
                to="/"
              >
                {t("campaigns.startRequest")}
              </Link>
            }
          />
        </Panel>
      )}
      {!error && rows.length > 0 && (
        <Panel className="campaigns-list">
          {rows.map(({ campaign, project }) => (
            <article className="campaign-row" key={campaign.id}>
              <div className="campaign-row-main">
                <div className="campaign-row-title">
                  <Link to={`/campaigns/${campaign.id}`}>
                    {campaign.objective ?? t("campaigns.untitled")}
                  </Link>
                  <Badge
                    tone={stateTone(
                      campaign.effective_status ?? campaign.status,
                    )}
                  >
                    {t(
                      stateLabelKey(
                        campaign.effective_status ?? campaign.status,
                      ),
                    )}
                  </Badge>
                </div>
                <p className="muted">
                  {project
                    ? projectDisplayName(project)
                    : t("campaigns.unknownProject")}
                  {" · "}
                  {t("campaigns.updatedShort", {
                    date: formatDate(campaign.created_at),
                  })}
                </p>
              </div>
              <div className="campaign-row-actions">
                <Link
                  className="ui-button ui-button--secondary ui-button--sm"
                  to={`/campaigns/${campaign.id}`}
                >
                  {t("campaigns.open")}
                </Link>
                <CancelCampaign
                  campaign={campaign}
                  onCancelled={() => void load()}
                />
              </div>
            </article>
          ))}
        </Panel>
      )}
    </div>
  );
}
