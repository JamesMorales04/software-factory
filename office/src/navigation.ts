import type { LucideIcon } from "lucide-react";
import {
  Activity,
  Bot,
  FolderGit2,
  Gauge,
  House,
  Inbox,
  ListChecks,
  Megaphone,
  MonitorSmartphone,
  Play,
  Route,
  Settings,
  ShieldCheck,
  Users,
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
  collapsible?: boolean;
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
        labelKey: "nav.home",
        icon: House,
        keywords: "home request compose objective start work intake",
      },
      {
        to: "/projects",
        labelKey: "nav.projects",
        icon: FolderGit2,
        keywords: "repositories imports configuration",
      },
      {
        to: "/campaigns",
        labelKey: "nav.campaigns",
        icon: Megaphone,
        keywords: "runs attempts candidates gate quality",
      },
      {
        to: "/issues",
        labelKey: "nav.issues",
        icon: ListChecks,
        keywords: "github issues sync repository labels triage backlog intake",
      },
      {
        to: "/inbox",
        labelKey: "nav.decisions",
        icon: Inbox,
        keywords:
          "decision inbox approvals blocked unresolved usage needs action",
      },
      {
        to: "/agents",
        labelKey: "nav.agents",
        icon: Bot,
        keywords:
          "agent office participants sessions implementer lead verifier reviewer worker tool events",
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
  {
    labelKey: "nav.group.advanced",
    collapsible: true,
    items: [
      {
        to: "/overview",
        labelKey: "nav.overview",
        icon: Gauge,
        keywords: "overview health activity events status metrics operations",
      },
      {
        to: "/actions",
        labelKey: "nav.actions",
        icon: ListChecks,
        keywords: "objectives specs checks risk frozen action",
      },
      {
        to: "/lead",
        labelKey: "nav.lead",
        icon: Route,
        keywords: "assessment plan freeze authorize legacy",
      },
      {
        to: "/delegations",
        labelKey: "nav.delegations",
        icon: ShieldCheck,
        keywords: "scoped grants budgets revoke",
      },
      {
        to: "/runs",
        labelKey: "nav.runs",
        icon: Play,
        keywords: "attempts queue execution",
      },
      {
        to: "/workers",
        labelKey: "nav.workers",
        icon: Users,
        keywords: "registered credentials",
      },
      {
        to: "/runtime",
        labelKey: "nav.runtime",
        icon: Activity,
        keywords: "provider sessions model calls telemetry cleanup",
      },
      {
        to: "/settings?tab=access",
        labelKey: "nav.accessDevices",
        icon: MonitorSmartphone,
        keywords:
          "access devices sessions pairing tailscale remote trust revoke",
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
  if (item.to === "/settings" && matchesQuery(search, "tab=access")) {
    return false;
  }
  if (targetPath === "/") {
    return pathname === "/" || pathname.startsWith("/requests/");
  }
  return pathname === targetPath || pathname.startsWith(`${targetPath}/`);
}

export function isAdvancedRoute(pathname: string, search = ""): boolean {
  const advanced = NAV_GROUPS.find((group) => group.collapsible);
  return (
    advanced?.items.some((item) => isNavItemActive(item, pathname, search)) ??
    false
  );
}

export function routeLabel(
  pathname: string,
  search: string,
  t: Translate,
): string {
  if (pathname.startsWith("/requests/")) return t("nav.request");
  if (pathname.startsWith("/projects/") && pathname.endsWith("/files"))
    return t("nav.projectFiles");
  if (pathname.startsWith("/campaigns/")) return t("nav.campaignDetail");
  if (pathname.startsWith("/actions/")) return t("nav.actionDetail");
  if (pathname.startsWith("/agents/")) return t("nav.agentParticipant");
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
