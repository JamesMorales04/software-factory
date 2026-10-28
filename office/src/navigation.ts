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

export interface NavItem {
  to: string;
  label: string;
  icon: LucideIcon;
  keywords: string;
}

export interface NavGroup {
  label: string;
  items: NavItem[];
  collapsible?: boolean;
}

/**
 * Operator-intent navigation. The primary group answers "what do I want to do
 * next"; the advanced group keeps every existing operational view reachable
 * without presenting internal domain objects as the first-run experience.
 */
export const NAV_GROUPS: NavGroup[] = [
  {
    label: "Workspace",
    items: [
      {
        to: "/",
        label: "Home",
        icon: House,
        keywords: "home request compose objective start work intake",
      },
      {
        to: "/projects",
        label: "Projects",
        icon: FolderGit2,
        keywords: "repositories imports configuration",
      },
      {
        to: "/campaigns",
        label: "Campaigns",
        icon: Megaphone,
        keywords: "runs attempts candidates gate quality",
      },
      {
        to: "/issues",
        label: "Issues",
        icon: ListChecks,
        keywords: "github issues sync repository labels triage backlog intake",
      },
      {
        to: "/inbox",
        label: "Decisions",
        icon: Inbox,
        keywords:
          "decision inbox approvals blocked unresolved usage needs action",
      },
      {
        to: "/agents",
        label: "Agents",
        icon: Bot,
        keywords:
          "agent office participants sessions implementer lead verifier reviewer worker tool events",
      },
      {
        to: "/settings",
        label: "Settings",
        icon: Settings,
        keywords:
          "providers models credentials runtime policy profiles access devices sessions tailscale pairing",
      },
    ],
  },
  {
    label: "Advanced",
    collapsible: true,
    items: [
      {
        to: "/overview",
        label: "Overview",
        icon: Gauge,
        keywords: "overview health activity events status metrics operations",
      },
      {
        to: "/actions",
        label: "Actions",
        icon: ListChecks,
        keywords: "objectives specs checks risk frozen action",
      },
      {
        to: "/lead",
        label: "Lead",
        icon: Route,
        keywords: "assessment plan freeze authorize legacy",
      },
      {
        to: "/delegations",
        label: "Delegations",
        icon: ShieldCheck,
        keywords: "scoped grants budgets revoke",
      },
      {
        to: "/runs",
        label: "Runs",
        icon: Play,
        keywords: "attempts queue execution",
      },
      {
        to: "/workers",
        label: "Workers",
        icon: Users,
        keywords: "registered credentials",
      },
      {
        to: "/runtime",
        label: "Runtime",
        icon: Activity,
        keywords: "provider sessions model calls telemetry cleanup",
      },
      {
        to: "/settings?tab=access",
        label: "Access & Devices",
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

export function routeLabel(pathname: string, search = ""): string {
  if (pathname.startsWith("/requests/")) return "Request";
  if (pathname.startsWith("/projects/") && pathname.endsWith("/files"))
    return "Project files";
  if (pathname.startsWith("/campaigns/")) return "Campaign detail";
  if (pathname.startsWith("/actions/")) return "Action detail";
  if (pathname.startsWith("/agents/")) return "Agent Office participant";
  if (pathname === "/settings" && matchesQuery(search, "tab=access")) {
    return "Access & Devices";
  }
  return (
    NAV_ITEMS.find(
      (item) =>
        item.to.split("?")[0] === pathname &&
        !item.to.includes("?") &&
        isNavItemActive(item, pathname, search),
    )?.label ?? "Factory Office"
  );
}
