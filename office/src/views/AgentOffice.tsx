import { useCallback, useEffect, useRef, useState } from "react";
import {
  Link,
  useNavigate,
  useParams,
  useSearchParams,
} from "react-router-dom";
import {
  AgentOfficeTable,
  AgentOfficeTimeline,
} from "../components/office/AgentOfficeFloor";
import { AgentOfficeIsometric } from "../components/office/AgentOfficeIsometric";
import { ParticipantInspector } from "../components/office/ParticipantInspector";
import { StatusBadge } from "../components/office/StatusBadge";
import { Button, buttonClass } from "../components/ui/Button";
import { Select } from "../components/ui/Field";
import {
  EmptyState,
  ErrorState,
  LoadingState,
  PageHeader,
  Panel,
} from "../components/ui/Surface";
import { Tabs } from "../components/ui/Tabs";
import {
  type Campaign,
  CoreApiError,
  coreApi,
  type ParticipantPage,
  type ParticipantRecord,
  type Project,
} from "../coreApi";
import { shortId } from "../format";
import { useI18n } from "../i18n";

const VIEW_IDS = ["floor", "table", "timeline"] as const;
type ViewId = (typeof VIEW_IDS)[number];

const VIEW_KEYS = [
  { id: "floor", labelKey: "agentOffice.views.floor" },
  { id: "table", labelKey: "agentOffice.views.table" },
  { id: "timeline", labelKey: "agentOffice.views.timeline" },
] as const;

const ROLE_OPTIONS = [
  ["IMPLEMENTER", "agentOffice.role.IMPLEMENTER"],
  ["LEAD", "agentOffice.role.LEAD"],
  ["VERIFIER", "agentOffice.role.VERIFIER"],
  ["REVIEWER", "agentOffice.role.REVIEWER"],
  ["WORKER", "agentOffice.role.WORKER"],
] as const;

const STATUS_GROUPS: {
  labelKey:
    | "agentOffice.statusGroup.providerSession"
    | "agentOffice.statusGroup.leadRequest"
    | "agentOffice.statusGroup.qualityReceipt"
    | "agentOffice.statusGroup.worker";
  values: string[];
}[] = [
  {
    labelKey: "agentOffice.statusGroup.providerSession",
    values: [
      "INTENT",
      "ACTIVE",
      "CANCEL_REQUESTED",
      "COMPLETED",
      "FAILED",
      "CANCELLED",
      "CLEANED",
      "ORPHANED",
    ],
  },
  {
    labelKey: "agentOffice.statusGroup.leadRequest",
    values: [
      "SUBMITTED",
      "ASSESSING",
      "PLANNED",
      "FROZEN",
      "READY",
      "RUNNING",
      "BLOCKED",
      "FAILED",
    ],
  },
  {
    labelKey: "agentOffice.statusGroup.qualityReceipt",
    values: ["PASS", "FAIL", "CHANGES_REQUESTED", "NOT_TESTED"],
  },
  { labelKey: "agentOffice.statusGroup.worker", values: ["REGISTERED"] },
];

const KNOWN_PROVIDERS = ["opencode", "opencode-go", "codex", "fixture"];

function resolveView(value: string | null): ViewId {
  return VIEW_IDS.includes(value as ViewId) ? (value as ViewId) : "floor";
}

function uniqueProviders(participants: ParticipantRecord[]): string[] {
  const values = new Set(KNOWN_PROVIDERS);
  for (const participant of participants) {
    const route = participant.requested_route ?? participant.observed_route;
    if (route?.provider) values.add(route.provider);
  }
  return [...values].sort();
}

export function AgentOffice() {
  const { t, formatDate, formatNumber } = useI18n();
  const { participantId } = useParams();
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const view = resolveView(searchParams.get("view"));
  const role = searchParams.get("role") ?? "";
  const status = searchParams.get("status") ?? "";
  const provider = searchParams.get("provider") ?? "";
  const campaignId = searchParams.get("campaign") ?? "";
  const projectId = searchParams.get("project") ?? "";
  const [page, setPage] = useState<ParticipantPage | null>(null);
  const [campaigns, setCampaigns] = useState<Campaign[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [selectedExtra, setSelectedExtra] = useState<ParticipantRecord | null>(
    null,
  );
  const [selectedMissing, setSelectedMissing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [lastLoadedAt, setLastLoadedAt] = useState<string | null>(null);
  const loadSequence = useRef(0);
  const alive = useRef(true);
  const optionsLoaded = useRef(false);

  const load = useCallback(async () => {
    const sequence = ++loadSequence.current;
    const [nextPage, nextCampaigns, nextProjects] = await Promise.all([
      coreApi.listParticipants({
        role: role || undefined,
        status: status || undefined,
        provider: provider || undefined,
        campaignId: campaignId || undefined,
        projectId: projectId || undefined,
        limit: 200,
      }),
      optionsLoaded.current
        ? Promise.resolve<Campaign[] | null>(null)
        : coreApi.listCampaigns(),
      optionsLoaded.current
        ? Promise.resolve<Project[] | null>(null)
        : coreApi.listProjects(),
    ]);
    let extra: ParticipantRecord | null = null;
    if (
      participantId &&
      !nextPage.participants.some(
        (participant) => participant.participant_id === participantId,
      )
    ) {
      const single = await coreApi.listParticipants({
        participantId,
        role: role || undefined,
        status: status || undefined,
        provider: provider || undefined,
        campaignId: campaignId || undefined,
        projectId: projectId || undefined,
        limit: 1,
      });
      extra = single.participants[0] ?? null;
    }
    if (sequence !== loadSequence.current || !alive.current) return;
    if (nextCampaigns) {
      setCampaigns(nextCampaigns);
    }
    if (nextProjects) {
      setProjects(nextProjects);
    }
    optionsLoaded.current = true;
    setSelectedExtra(extra);
    setSelectedMissing(
      Boolean(participantId) &&
        extra === null &&
        !nextPage.participants.some(
          (participant) => participant.participant_id === participantId,
        ),
    );
    setPage(nextPage);
    setLastLoadedAt(new Date().toISOString());
    setError(null);
  }, [campaignId, participantId, projectId, provider, role, status]);

  useEffect(() => {
    alive.current = true;
    async function refresh() {
      try {
        await load();
      } catch (cause) {
        if (!alive.current) return;
        setError(
          cause instanceof CoreApiError
            ? cause.message
            : t("agentOffice.error.load"),
        );
      }
    }
    void refresh();
    const timer = setInterval(() => void refresh(), 5000);
    return () => {
      alive.current = false;
      clearInterval(timer);
    };
  }, [load, t]);

  const allParticipants = page?.participants ?? [];
  const selected =
    allParticipants.find(
      (participant) => participant.participant_id === participantId,
    ) ??
    (selectedExtra?.participant_id === participantId ? selectedExtra : null);
  const participants =
    selectedExtra &&
    !allParticipants.some(
      (participant) =>
        participant.participant_id === selectedExtra.participant_id,
    )
      ? [...allParticipants, selectedExtra]
      : allParticipants;
  const staleCount = participants.filter(
    (participant) => participant.observation === "STALE",
  ).length;
  const currentCount = participants.filter(
    (participant) => participant.observation === "CURRENT",
  ).length;

  const setView = (next: string) => {
    const params = new URLSearchParams(searchParams);
    params.set("view", resolveView(next));
    setSearchParams(params);
  };

  const setFilter = (key: string, value: string) => {
    const params = new URLSearchParams(searchParams);
    if (value === "") params.delete(key);
    else params.set(key, value);
    setSearchParams(params);
    if (participantId) {
      navigate({ pathname: "/agents", search: params.toString() });
    }
  };

  const selectParticipant = (id: string) => {
    navigate({
      pathname: `/agents/${encodeURIComponent(id)}`,
      search: searchParams.toString(),
    });
  };

  const closeInspector = () => {
    navigate({ pathname: "/agents", search: searchParams.toString() });
  };

  const clearFilters = () => {
    const params = new URLSearchParams();
    if (view !== "floor") params.set("view", view);
    setSearchParams(params);
    navigate({ pathname: "/agents", search: params.toString() });
  };

  const hasFilters = Boolean(
    role || status || provider || campaignId || projectId,
  );
  const providers = uniqueProviders(participants);

  if (error && !page) {
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
  if (!page) return <LoadingState label={t("agentOffice.loading")} />;

  return (
    <div className="agent-office">
      <PageHeader
        title={t("agentOffice.title")}
        description={t("agentOffice.description")}
        meta={
          <>
            <span>
              {t(
                participants.length === 1
                  ? "agentOffice.meta.participantsOne"
                  : "agentOffice.meta.participantsOther",
                { count: formatNumber(participants.length) },
              )}
            </span>
            <span className="overview-meta-separator" aria-hidden="true">
              /
            </span>
            <span>
              {t(
                currentCount === 1
                  ? "agentOffice.meta.currentOne"
                  : "agentOffice.meta.currentOther",
                { count: formatNumber(currentCount) },
              )}
            </span>
            <span className="overview-meta-separator" aria-hidden="true">
              /
            </span>
            <span>
              {t("agentOffice.meta.stale", {
                count: formatNumber(staleCount),
              })}
            </span>
            {lastLoadedAt && (
              <>
                <span className="overview-meta-separator" aria-hidden="true">
                  /
                </span>
                <span>
                  {t("agentOffice.meta.snapshot", {
                    date: formatDate(lastLoadedAt),
                  })}
                </span>
              </>
            )}
          </>
        }
        actions={
          <>
            <Link className={buttonClass("secondary")} to="/campaigns">
              {t("nav.campaigns")}
            </Link>
            <Link className={buttonClass("quiet")} to="/runtime">
              {t("nav.runtime")}
            </Link>
          </>
        }
      />

      {error && (
        <div className="callout callout-attention" role="status">
          <strong>{t("agentOffice.error.refresh")}</strong>{" "}
          {t("agentOffice.error.refreshBody", {
            from: lastLoadedAt
              ? t("agentOffice.error.from", {
                  date: formatDate(lastLoadedAt),
                })
              : "",
          })}
        </div>
      )}
      {page.page.has_more && (
        <div className="callout" role="status">
          {t("agentOffice.page.more", {
            count: formatNumber(page.page.returned),
          })}
        </div>
      )}
      {staleCount > 0 && (
        <div className="callout callout-attention" role="status">
          <strong>
            {t(
              staleCount === 1
                ? "agentOffice.stale.one"
                : "agentOffice.stale.other",
              { count: formatNumber(staleCount) },
            )}
          </strong>{" "}
          {t("agentOffice.stale.body")}
        </div>
      )}

      <Panel className="agent-filter-panel">
        <div className="agent-filters" data-testid="agent-filters">
          <Select
            label={t("agentOffice.filters.role")}
            value={role}
            onChange={(event) => setFilter("role", event.target.value)}
          >
            <option value="">{t("agentOffice.filters.allRoles")}</option>
            {ROLE_OPTIONS.map(([value, labelKey]) => (
              <option value={value} key={value}>
                {t(labelKey)}
              </option>
            ))}
          </Select>
          <Select
            label={t("agentOffice.filters.status")}
            value={status}
            onChange={(event) => setFilter("status", event.target.value)}
          >
            <option value="">{t("agentOffice.filters.allStatuses")}</option>
            {STATUS_GROUPS.map((group) => (
              <optgroup label={t(group.labelKey)} key={group.labelKey}>
                {group.values.map((value) => (
                  <option value={value} key={`${group.labelKey}-${value}`}>
                    {value}
                  </option>
                ))}
              </optgroup>
            ))}
          </Select>
          <Select
            label={t("agentOffice.filters.provider")}
            value={provider}
            onChange={(event) => setFilter("provider", event.target.value)}
          >
            <option value="">{t("agentOffice.filters.allProviders")}</option>
            {providers.map((value) => (
              <option value={value} key={value}>
                {value}
              </option>
            ))}
          </Select>
          <Select
            label={t("agentOffice.filters.campaign")}
            value={campaignId}
            onChange={(event) => setFilter("campaign", event.target.value)}
          >
            <option value="">{t("agentOffice.filters.allCampaigns")}</option>
            {[...campaigns]
              .sort((left, right) =>
                right.created_at.localeCompare(left.created_at),
              )
              .map((campaign) => (
                <option value={campaign.id} key={campaign.id}>
                  {shortId(campaign.id, 12)} · {campaign.status}
                </option>
              ))}
          </Select>
          <Select
            label={t("agentOffice.filters.project")}
            help={t("agentOffice.filters.projectHelp")}
            value={projectId}
            onChange={(event) => setFilter("project", event.target.value)}
          >
            <option value="">{t("agentOffice.filters.allProjects")}</option>
            {projects.map((project) => (
              <option value={project.id} key={project.id}>
                {shortId(project.id, 12)} · {project.repository}
              </option>
            ))}
          </Select>
          <div className="agent-filter-action">
            <Button
              variant="quiet"
              onClick={clearFilters}
              disabled={!hasFilters}
            >
              {t("agentOffice.filters.clear")}
            </Button>
          </div>
        </div>
        <p className="muted">{t("agentOffice.filters.note")}</p>
      </Panel>

      <div className="agent-workspace">
        <div className="agent-main">
          <div className="section-heading">
            <h2>{t("agentOffice.participants.title")}</h2>
            <span className="muted">
              {t("agentOffice.participants.meta", {
                count: formatNumber(participants.length),
              })}
            </span>
          </div>
          <Tabs
            items={VIEW_KEYS.map((item) => ({
              id: item.id,
              label: t(item.labelKey),
              count: item.id === view ? participants.length : undefined,
            }))}
            value={view}
            onChange={setView}
            label={t("agentOffice.views.label")}
            className="agent-view-tabs"
          />
          <div
            role="tabpanel"
            id={`panel-${view}`}
            aria-labelledby={`tab-${view}`}
            className="agent-view-panel"
          >
            {participants.length === 0 ? (
              <Panel>
                <EmptyState
                  title={
                    hasFilters
                      ? t("agentOffice.emptyFilteredTitle")
                      : t("agentOffice.emptyTitle")
                  }
                  description={
                    hasFilters
                      ? t("agentOffice.emptyFilteredDescription")
                      : t("agentOffice.emptyDescription")
                  }
                  action={
                    hasFilters ? (
                      <Button onClick={clearFilters}>
                        {t("agentOffice.filters.clear")}
                      </Button>
                    ) : (
                      <Link
                        className={buttonClass("secondary")}
                        to="/campaigns"
                      >
                        {t("agentOffice.openCampaigns")}
                      </Link>
                    )
                  }
                />
              </Panel>
            ) : view === "floor" ? (
              <AgentOfficeIsometric
                participants={participants}
                selectedId={participantId ?? null}
                onSelect={selectParticipant}
              />
            ) : view === "table" ? (
              <AgentOfficeTable
                participants={participants}
                selectedId={participantId ?? null}
                onSelect={selectParticipant}
              />
            ) : (
              <AgentOfficeTimeline
                participants={participants}
                selectedId={participantId ?? null}
                onSelect={selectParticipant}
              />
            )}
          </div>

          <details className="agent-sources">
            <summary>{t("agentOffice.sources.summary")}</summary>
            <ul>
              {page.sources.map((source) => (
                <li key={source.role}>
                  <strong>{source.role}</strong> — {source.source}.{" "}
                  <span className="muted">{source.note}</span>
                </li>
              ))}
            </ul>
            <p className="muted">
              {t("agentOffice.sources.note")} <code>provider_tool_events</code>{" "}
              {t("agentOffice.sources.noteEnd")}
            </p>
          </details>
        </div>

        <div className="agent-side">
          {selectedMissing && !selected ? (
            <Panel className="agent-inspector" as="aside">
              <EmptyState
                title={t("agentOffice.participantMissingTitle")}
                description={t("agentOffice.participantMissingDescription")}
                action={
                  <Button onClick={clearFilters}>
                    {t("agentOffice.filters.clear")}
                  </Button>
                }
              />
            </Panel>
          ) : (
            <ParticipantInspector
              participant={selected}
              onClose={closeInspector}
            />
          )}
          <Panel className="agent-legend">
            <h3>{t("agentOffice.legend.title")}</h3>
            <ul>
              <li>
                <StatusBadge status="CURRENT" />{" "}
                {t("agentOffice.legend.current")}
              </li>
              <li>
                <StatusBadge status="STALE" /> {t("agentOffice.legend.stale")}
              </li>
              <li>
                <StatusBadge status="FINAL" /> {t("agentOffice.legend.final")}
              </li>
              <li>
                <StatusBadge status="NOT_EXPOSED" label="NOT EXPOSED" />{" "}
                {t("agentOffice.legend.notExposed")}
              </li>
            </ul>
          </Panel>
        </div>
      </div>
    </div>
  );
}
