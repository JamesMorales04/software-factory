import { useCallback, useEffect, useRef, useState } from "react";
import {
  Link,
  useNavigate,
  useParams,
  useSearchParams,
} from "react-router-dom";
import {
  AgentOfficeFloor,
  AgentOfficeTable,
  AgentOfficeTimeline,
} from "../components/office/AgentOfficeFloor";
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
import { type TabItem, Tabs } from "../components/ui/Tabs";
import {
  type Campaign,
  CoreApiError,
  coreApi,
  type ParticipantPage,
  type ParticipantRecord,
  type Project,
} from "../coreApi";
import { formatTimestamp, shortId } from "../format";

const VIEW_IDS = ["floor", "table", "timeline"] as const;
type ViewId = (typeof VIEW_IDS)[number];

const VIEW_ITEMS: TabItem[] = [
  { id: "floor", label: "Visual Office" },
  { id: "table", label: "Table" },
  { id: "timeline", label: "Timeline" },
];

const ROLE_OPTIONS = [
  ["IMPLEMENTER", "Implementer"],
  ["LEAD", "Lead"],
  ["VERIFIER", "Verifier"],
  ["REVIEWER", "Reviewer"],
  ["WORKER", "Worker"],
] as const;

const STATUS_GROUPS: { label: string; values: string[] }[] = [
  {
    label: "Provider session",
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
    label: "Lead request",
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
    label: "Quality receipt",
    values: ["PASS", "FAIL", "CHANGES_REQUESTED", "NOT_TESTED"],
  },
  { label: "Worker", values: ["REGISTERED"] },
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
            : "Failed to load the Agent Office projection.",
        );
      }
    }
    void refresh();
    const timer = setInterval(() => void refresh(), 5000);
    return () => {
      alive.current = false;
      clearInterval(timer);
    };
  }, [load]);

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
        message={error}
        action={<Button onClick={() => window.location.reload()}>Retry</Button>}
      />
    );
  }
  if (!page) return <LoadingState label="Loading Agent Office…" />;

  return (
    <div className="agent-office">
      <PageHeader
        title="Agent Office"
        description="Recorded engineering participants projected from Core sessions, Lead requests, independent receipts and Worker registrations. No simulated movement, reasoning or tool activity."
        meta={
          <>
            <span>
              {participants.length} participant
              {participants.length === 1 ? "" : "s"} in this projection
            </span>
            <span className="overview-meta-separator" aria-hidden="true">
              /
            </span>
            <span>
              {currentCount} current observation{currentCount === 1 ? "" : "s"}
            </span>
            <span className="overview-meta-separator" aria-hidden="true">
              /
            </span>
            <span>{staleCount} stale unresolved</span>
            {lastLoadedAt && (
              <>
                <span className="overview-meta-separator" aria-hidden="true">
                  /
                </span>
                <span>Snapshot {formatTimestamp(lastLoadedAt)}</span>
              </>
            )}
          </>
        }
        actions={
          <>
            <Link className={buttonClass("secondary")} to="/campaigns">
              Campaigns
            </Link>
            <Link className={buttonClass("quiet")} to="/runtime">
              Runtime
            </Link>
          </>
        }
      />

      {error && (
        <div className="callout callout-attention" role="status">
          <strong>Core refresh failed.</strong> {error} Showing the last
          authoritative snapshot
          {lastLoadedAt ? ` from ${formatTimestamp(lastLoadedAt)}` : ""}. A
          disconnected view does not mean a participant stopped.
        </div>
      )}
      {page.page.has_more && (
        <div className="callout" role="status">
          Showing the first {page.page.returned} participants. The projection is
          bounded; refine the Campaign, role, provider or status filters to
          inspect more. No records were hidden silently.
        </div>
      )}
      {staleCount > 0 && (
        <div className="callout callout-attention" role="status">
          <strong>
            {staleCount} unresolved participant observation
            {staleCount === 1 ? " is" : "s are"} stale.
          </strong>{" "}
          The recorded session has no matching live lease. Review cleanup; this
          is not proof that the external session stopped.
        </div>
      )}

      <Panel className="agent-filter-panel">
        <div className="agent-filters" data-testid="agent-filters">
          <Select
            label="Role"
            value={role}
            onChange={(event) => setFilter("role", event.target.value)}
          >
            <option value="">All roles</option>
            {ROLE_OPTIONS.map(([value, label]) => (
              <option value={value} key={value}>
                {label}
              </option>
            ))}
          </Select>
          <Select
            label="Recorded status"
            value={status}
            onChange={(event) => setFilter("status", event.target.value)}
          >
            <option value="">All statuses</option>
            {STATUS_GROUPS.map((group) => (
              <optgroup label={group.label} key={group.label}>
                {group.values.map((value) => (
                  <option value={value} key={`${group.label}-${value}`}>
                    {value}
                  </option>
                ))}
              </optgroup>
            ))}
          </Select>
          <Select
            label="Provider"
            value={provider}
            onChange={(event) => setFilter("provider", event.target.value)}
          >
            <option value="">All providers</option>
            {providers.map((value) => (
              <option value={value} key={value}>
                {value}
              </option>
            ))}
          </Select>
          <Select
            label="Campaign"
            value={campaignId}
            onChange={(event) => setFilter("campaign", event.target.value)}
          >
            <option value="">All Campaigns</option>
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
            label="Project"
            help="Projection scope only. Operator authorization remains Factory-wide."
            value={projectId}
            onChange={(event) => setFilter("project", event.target.value)}
          >
            <option value="">All Projects</option>
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
              Clear filters
            </Button>
          </div>
        </div>
        <p className="muted">
          Filters are applied by Core to the participant projection. They do not
          grant or restrict operator authority.
        </p>
      </Panel>

      <div className="agent-workspace">
        <div className="agent-main">
          <div className="section-heading">
            <h2>Participants</h2>
            <span className="muted">
              {participants.length} shown · ordered by last recorded observation
            </span>
          </div>
          <Tabs
            items={VIEW_ITEMS.map((item) => ({
              ...item,
              count: item.id === view ? participants.length : undefined,
            }))}
            value={view}
            onChange={setView}
            label="Agent Office views"
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
                      ? "No matching participants"
                      : "No recorded participants"
                  }
                  description={
                    hasFilters
                      ? "No authoritative participant record matches the current filters. Clear a filter or inspect a different Campaign."
                      : "Core has no recorded Lead, Implementer, Verifier, Reviewer or Worker participant yet. Start an authorized Campaign to populate the Office."
                  }
                  action={
                    hasFilters ? (
                      <Button onClick={clearFilters}>Clear filters</Button>
                    ) : (
                      <Link
                        className={buttonClass("secondary")}
                        to="/campaigns"
                      >
                        Open Campaigns
                      </Link>
                    )
                  }
                />
              </Panel>
            ) : view === "floor" ? (
              <AgentOfficeFloor
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
            <summary>Projection sources and evidence levels</summary>
            <ul>
              {page.sources.map((source) => (
                <li key={source.role}>
                  <strong>{source.role}</strong> — {source.source}.{" "}
                  <span className="muted">{source.note}</span>
                </li>
              ))}
            </ul>
            <p className="muted">
              Provider tool events are read from the existing{" "}
              <code>provider_tool_events</code> ledger through a bounded,
              redacted Core projection. Office polling is not an event stream.
            </p>
          </details>
        </div>

        <div className="agent-side">
          {selectedMissing && !selected ? (
            <Panel className="agent-inspector" as="aside">
              <EmptyState
                title="Participant not found"
                description="The requested participant identity is not present in the current Core projection. It may be filtered by Campaign scope or no longer recorded."
                action={<Button onClick={clearFilters}>Clear filters</Button>}
              />
            </Panel>
          ) : (
            <ParticipantInspector
              participant={selected}
              onClose={closeInspector}
            />
          )}
          <Panel className="agent-legend">
            <h3>Truthful state</h3>
            <ul>
              <li>
                <StatusBadge status="CURRENT" /> Core holds a matching live
                lease generation.
              </li>
              <li>
                <StatusBadge status="STALE" /> The session is unresolved but no
                live lease matches it.
              </li>
              <li>
                <StatusBadge status="FINAL" /> The recorded lifecycle is
                terminal.
              </li>
              <li>
                <StatusBadge status="NOT_EXPOSED" label="NOT EXPOSED" /> Core
                does not record the requested fact.
              </li>
            </ul>
          </Panel>
        </div>
      </div>
    </div>
  );
}
