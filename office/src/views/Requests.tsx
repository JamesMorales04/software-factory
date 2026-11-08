import { useCallback, useEffect, useMemo, useState } from "react";
import { Link, useSearchParams } from "react-router-dom";
import { Button } from "../components/ui/Button";
import { Select } from "../components/ui/Field";
import {
  EmptyState,
  ErrorState,
  LoadingState,
  PageHeader,
  Panel,
} from "../components/ui/Surface";
import {
  CoreApiError,
  coreApi,
  type IntakeRequestSummary,
  type IntakeState,
  type Project,
} from "../coreApi";
import { useI18n } from "../i18n";
import { projectDisplayName } from "../workspace";
import { IntakeStateBadge } from "./RequestState";
import "./requests.css";

const STATES: IntakeState[] = [
  "SUBMITTED",
  "ASSESSING",
  "PLAN_READY",
  "AWAITING_APPROVAL",
  "EXECUTING",
  "BLOCKED",
  "FAILED",
  "COMPLETED",
  "CANCELLED",
];

function errorText(cause: unknown, fallback: string): string {
  return cause instanceof CoreApiError ? cause.message : fallback;
}

export function Requests() {
  const { t, formatDate } = useI18n();
  const [searchParams, setSearchParams] = useSearchParams();
  const projectId = searchParams.get("project") ?? "";
  const state = (searchParams.get("state") ?? "") as IntakeState | "";
  const [projects, setProjects] = useState<Project[]>([]);
  const [requests, setRequests] = useState<IntakeRequestSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [lastLoadedAt, setLastLoadedAt] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const [nextProjects, nextRequests] = await Promise.all([
        coreApi.listProjects(),
        coreApi.listIntakeRequests({
          projectId: projectId || undefined,
          state: state || undefined,
          limit: 100,
        }),
      ]);
      setProjects(nextProjects);
      setRequests(nextRequests);
      setError(null);
      setLastLoadedAt(new Date().toISOString());
    } catch (cause) {
      setError(errorText(cause, t("requests.error.load")));
    }
  }, [projectId, state, t]);

  useEffect(() => {
    void load();
    const timer = window.setInterval(() => void load(), 8000);
    return () => window.clearInterval(timer);
  }, [load]);

  const projectById = useMemo(
    () => new Map(projects.map((project) => [project.id, project])),
    [projects],
  );

  function setFilter(key: "project" | "state", value: string) {
    setSearchParams(
      (current) => {
        const next = new URLSearchParams(current);
        if (value) next.set(key, value);
        else next.delete(key);
        return next;
      },
      { replace: true },
    );
  }

  return (
    <div className="requests-workspace">
      <PageHeader
        title={t("requests.title")}
        description={t("requests.description")}
        meta={
          lastLoadedAt && (
            <span>
              {t("requests.updated", { date: formatDate(lastLoadedAt) })}
            </span>
          )
        }
        actions={
          <Link className="ui-button ui-button--primary ui-button--md" to="/">
            {t("requests.new")}
          </Link>
        }
      />

      <Panel className="requests-filters">
        <Select
          label={t("requests.filter.project")}
          value={projectId}
          onChange={(event) => setFilter("project", event.target.value)}
        >
          <option value="">{t("requests.filter.allProjects")}</option>
          {projects.map((project) => (
            <option key={project.id} value={project.id}>
              {projectDisplayName(project)}
            </option>
          ))}
        </Select>
        <Select
          label={t("requests.filter.state")}
          value={state}
          onChange={(event) => setFilter("state", event.target.value)}
        >
          <option value="">{t("requests.filter.allStates")}</option>
          {STATES.map((item) => (
            <option key={item} value={item}>
              {t(`request.state.${item}`)}
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
      {!error && requests === null && (
        <LoadingState label={t("requests.loading")} />
      )}
      {!error && requests !== null && requests.length === 0 && (
        <Panel>
          <EmptyState
            title={t("requests.emptyTitle")}
            description={t("requests.emptyDescription")}
            action={
              <Link
                className="ui-button ui-button--primary ui-button--md"
                to="/"
              >
                {t("requests.start")}
              </Link>
            }
          />
        </Panel>
      )}
      {!error && requests !== null && requests.length > 0 && (
        <Panel className="requests-list">
          {requests.map((request) => {
            const project = projectById.get(request.projectId);
            return (
              <Link
                key={request.id}
                to={`/requests/${request.id}`}
                className="requests-row"
              >
                <span className="requests-row-main">
                  <strong>{request.objective}</strong>
                  <span className="muted">
                    {project
                      ? projectDisplayName(project)
                      : t("requests.unknownProject")}
                    {" · "}
                    {formatDate(request.updatedAt)}
                  </span>
                </span>
                <IntakeStateBadge state={request.state} />
              </Link>
            );
          })}
        </Panel>
      )}
    </div>
  );
}
