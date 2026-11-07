import { FolderGit2, ShieldCheck } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { coreApi, type ExecutionProfile, type Project } from "../coreApi";
import { useI18n } from "../i18n";
import {
  projectDisplayName,
  readWorkspaceSelection,
  subscribeWorkspaceSelection,
  type WorkspaceSelection,
  writeWorkspaceSelection,
} from "../workspace";

/**
 * Top-bar workspace defaults. The selectors only choose the Project and
 * Execution Profile used by new requests; Core remains the authorization
 * authority for every operation.
 */
export function WorkspaceContextBar() {
  const { t } = useI18n();
  const [selection, setSelection] = useState<WorkspaceSelection>(
    readWorkspaceSelection,
  );
  const [projects, setProjects] = useState<Project[] | null>(null);
  const [profiles, setProfiles] = useState<ExecutionProfile[] | null>(null);
  const [projectsFailed, setProjectsFailed] = useState(false);
  const [profilesFailed, setProfilesFailed] = useState(false);
  const hydrated = useRef(false);

  useEffect(() => {
    let active = true;
    void coreApi
      .listProjects()
      .then((next) => {
        if (!active) return;
        setProjects(next);
        setProjectsFailed(false);
      })
      .catch(() => {
        if (!active) return;
        setProjects([]);
        setProjectsFailed(true);
      });
    void coreApi
      .listExecutionProfiles()
      .then(({ profiles: next }) => {
        if (!active) return;
        setProfiles(next);
        setProfilesFailed(false);
      })
      .catch(() => {
        if (!active) return;
        setProfiles([]);
        setProfilesFailed(true);
      });
    return () => {
      active = false;
    };
  }, []);

  // Resolve stored defaults exactly once, when both Core catalogs are known.
  // User changes and cross-component events write through `writeWorkspaceSelection`;
  // they must not re-enter this hydration effect or a stale selection can
  // overwrite a newer operator choice.
  useEffect(() => {
    if (projects === null || profiles === null || hydrated.current) return;
    const stored = readWorkspaceSelection();
    const next: WorkspaceSelection = {
      projectId:
        [selection.projectId, stored.projectId].find((id) =>
          projects.some((project) => project.id === id),
        ) ??
        projects[0]?.id ??
        "",
      profileId:
        [selection.profileId, stored.profileId].find((id) =>
          profiles.some((profile) => profile.id === id),
        ) ??
        profiles.find((profile) => profile.globalDefault)?.id ??
        profiles.find((profile) => profile.executable)?.id ??
        "",
    };
    hydrated.current = true;
    setSelection(next);
    writeWorkspaceSelection(next);
  }, [projects, profiles, selection]);

  useEffect(
    () => subscribeWorkspaceSelection((next) => setSelection(next)),
    [],
  );

  function changeProject(projectId: string) {
    const next = { ...selection, projectId };
    setSelection(next);
    writeWorkspaceSelection(next);
  }

  function changeProfile(profileId: string) {
    const next = { ...selection, profileId };
    setSelection(next);
    writeWorkspaceSelection(next);
  }

  const projectPlaceholder =
    projects === null
      ? t("workspace.loading")
      : projectsFailed || projects.length === 0
        ? t("workspace.unavailable")
        : t("workspace.selectProject");
  const profilePlaceholder =
    profiles === null
      ? t("workspace.loading")
      : profilesFailed || profiles.length === 0
        ? t("workspace.unavailable")
        : t("workspace.selectProfile");

  return (
    <fieldset className="workspace-context" aria-label={t("workspace.context")}>
      <label className="workspace-context-field">
        <span className="workspace-context-label">
          <FolderGit2 size={14} aria-hidden="true" />
          {t("workspace.project")}
        </span>
        <select
          className="workspace-context-select"
          value={selection.projectId}
          onChange={(event) => changeProject(event.target.value)}
          disabled={projects === null || projects.length === 0}
          aria-label={t("workspace.projectAria")}
        >
          {projects === null || projects.length === 0 ? (
            <option value="">{projectPlaceholder}</option>
          ) : (
            projects.map((project) => (
              <option key={project.id} value={project.id}>
                {projectDisplayName(project)}
              </option>
            ))
          )}
        </select>
      </label>
      <label className="workspace-context-field">
        <span className="workspace-context-label">
          <ShieldCheck size={14} aria-hidden="true" />
          {t("workspace.quality")}
        </span>
        <select
          className="workspace-context-select"
          value={selection.profileId}
          onChange={(event) => changeProfile(event.target.value)}
          disabled={profiles === null || profiles.length === 0}
          aria-label={t("workspace.qualityAria")}
        >
          {profiles === null || profiles.length === 0 ? (
            <option value="">{profilePlaceholder}</option>
          ) : (
            profiles.map((profile) => (
              <option key={profile.id} value={profile.id}>
                {profile.name}
                {profile.globalDefault ? ` · ${t("workspace.default")}` : ""}
              </option>
            ))
          )}
        </select>
      </label>
    </fieldset>
  );
}
