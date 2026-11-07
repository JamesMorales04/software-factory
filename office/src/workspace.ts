/**
 * Office workspace context.
 *
 * The selected Project and Execution Profile are ordinary operator defaults for
 * new requests. They are not an authorization boundary: Core still authorizes
 * every request, Project, Campaign and provider call from its own frozen state.
 */
export const WORKSPACE_PROJECT_KEY = "factory-office-project";
export const WORKSPACE_PROFILE_KEY = "factory-office-profile";
export const WORKSPACE_CHANGE_EVENT = "factory-workspace-change";

export interface WorkspaceSelection {
  projectId: string;
  profileId: string;
}

export function readWorkspaceSelection(): WorkspaceSelection {
  if (typeof window === "undefined") {
    return { projectId: "", profileId: "" };
  }
  return {
    projectId: window.localStorage.getItem(WORKSPACE_PROJECT_KEY) ?? "",
    profileId: window.localStorage.getItem(WORKSPACE_PROFILE_KEY) ?? "",
  };
}

export function writeWorkspaceSelection(
  patch: Partial<WorkspaceSelection>,
): WorkspaceSelection {
  const current = readWorkspaceSelection();
  const next = { ...current, ...patch };
  if (
    current.projectId === next.projectId &&
    current.profileId === next.profileId
  ) {
    return next;
  }
  if (typeof window !== "undefined") {
    window.localStorage.setItem(WORKSPACE_PROJECT_KEY, next.projectId);
    window.localStorage.setItem(WORKSPACE_PROFILE_KEY, next.profileId);
    window.dispatchEvent(
      new CustomEvent<WorkspaceSelection>(WORKSPACE_CHANGE_EVENT, {
        detail: next,
      }),
    );
  }
  return next;
}

export function subscribeWorkspaceSelection(
  listener: (selection: WorkspaceSelection) => void,
): () => void {
  if (typeof window === "undefined") return () => undefined;
  const handler = (event: Event) => {
    const detail = (event as CustomEvent<WorkspaceSelection>).detail;
    listener(detail ?? readWorkspaceSelection());
  };
  window.addEventListener(WORKSPACE_CHANGE_EVENT, handler);
  return () => window.removeEventListener(WORKSPACE_CHANGE_EVENT, handler);
}

export function projectDisplayName(project: {
  name?: string;
  repository: string;
  id: string;
}): string {
  const name = project.name?.trim();
  if (name && name !== "") return name;
  const repository = project.repository.replace(/\/+$/, "");
  return repository.split("/").filter(Boolean).at(-1) ?? project.id;
}
