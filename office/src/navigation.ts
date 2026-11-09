import type { LucideIcon } from "lucide-react";
import {
  Activity,
  BookOpen,
  Bot,
  ClipboardList,
  FileText,
  FolderGit2,
  Inbox,
  ListChecks,
  Megaphone,
  Network,
  Settings,
  Sparkles,
} from "lucide-react";
import type { MessageKey, Translate } from "./i18n";

export interface NavItem {
  to: string;
  labelKey: MessageKey;
  icon: LucideIcon;
  keywords: string;
}

export interface NavGroup {
  labelKey: MessageKey;
  items: NavItem[];
}

/**
 * Operator-intent navigation. The primary group answers "what do I want to do
 * next"; the advanced group keeps every existing operational view reachable
 * without presenting internal domain objects as the first-run experience.
 * Labels are catalog keys so the shell, command palette and route titles share
 * one translation source.
 */
export const NAV_GROUPS: NavGroup[] = [
  {
    labelKey: "nav.group.workspace",
    items: [
      {
        to: "/",
        labelKey: "nav.newRequest",
        icon: Sparkles,
        keywords: "home request compose objective start work intake",
      },
      {
        to: "/requests",
        labelKey: "nav.requests",
        icon: ClipboardList,
        keywords: "requests history status objectives",
      },
      {
        to: "/director",
        labelKey: "nav.director",
        icon: Network,
        keywords:
          "director portfolio route assurance stack agents participants operation manager planning triage context handoff",
      },
      {
        to: "/inbox",
        labelKey: "nav.decisions",
        icon: Inbox,
        keywords:
          "decision inbox approvals blocked unresolved usage needs action",
      },
      {
        to: "/campaigns",
        labelKey: "nav.campaigns",
        icon: Megaphone,
        keywords: "work campaigns runs attempts candidates gate quality",
      },
      {
        to: "/projects",
        labelKey: "nav.projects",
        icon: FolderGit2,
        keywords: "repositories imports configuration",
      },
      {
        to: "/issues",
        labelKey: "nav.issues",
        icon: ListChecks,
        keywords: "github issues sync repository labels triage backlog intake",
      },
      {
        to: "/specs",
        labelKey: "nav.specs",
        icon: FileText,
        keywords:
          "specifications specs formal frozen workgraph work graph requirements acceptance checks kanban board github",
      },
      {
        to: "/skills",
        labelKey: "nav.skills",
        icon: BookOpen,
        keywords:
          "skills knowledge context7 librarian explorer library docs temporary promotion license review revoke",
      },
      {
        to: "/agents",
        labelKey: "nav.agentOffice",
        icon: Bot,
        keywords:
          "agent office participants sessions implementer lead verifier reviewer worker tool events",
      },
      {
        to: "/runtime",
        labelKey: "nav.runtime",
        icon: Activity,
        keywords:
          "telemetry logs sessions model calls workers runs diagnostics usage tokens",
      },
      {
        to: "/settings",
        labelKey: "nav.settings",
        icon: Settings,
        keywords:
          "providers models credentials runtime policy profiles access devices sessions tailscale pairing",
      },
    ],
  },
];

export const NAV_ITEMS = NAV_GROUPS.flatMap((group) => group.items);

function matchesQuery(search: string, target: string): boolean {
  const current = new URLSearchParams(search);
  return [...new URLSearchParams(target)].every(
    ([key, value]) => current.get(key) === value,
  );
}

/**
 * A request workspace belongs to Home; detail routes keep their list section
 * active. Query-backed destinations (Access & Devices) are distinguishable
 * from their parent Settings page.
 */
export function isNavItemActive(
  item: NavItem,
  pathname: string,
  search = "",
): boolean {
  const [targetPath, targetQuery = ""] = item.to.split("?");
  if (targetQuery !== "") {
    return pathname === targetPath && matchesQuery(search, targetQuery);
  }
  if (targetPath === "/") {
    return pathname === "/";
  }
  return pathname === targetPath || pathname.startsWith(`${targetPath}/`);
}

export function routeLabel(
  pathname: string,
  search: string,
  t: Translate,
): string {
  if (pathname.startsWith("/requests/")) return t("nav.request");
  if (pathname.startsWith("/projects/") && pathname.endsWith("/files"))
    return t("nav.projectFiles");
  if (pathname.startsWith("/projects/")) return t("nav.projectDetail");
  if (pathname.startsWith("/campaigns/")) return t("nav.campaignDetail");
  if (pathname.startsWith("/agents/")) return t("nav.agentParticipant");
  if (pathname === "/director") return t("nav.director");
  if (pathname === "/settings" && matchesQuery(search, "tab=access")) {
    return t("nav.accessDevices");
  }
  const item = NAV_ITEMS.find(
    (candidate) =>
      candidate.to.split("?")[0] === pathname &&
      !candidate.to.includes("?") &&
      isNavItemActive(candidate, pathname, search),
  );
  return item ? t(item.labelKey) : t("nav.factoryOffice");
}
