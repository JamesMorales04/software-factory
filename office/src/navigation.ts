import type { LucideIcon } from "lucide-react";
import {
  Activity,
  Bot,
  FolderGit2,
  Gauge,
  Inbox,
  ListChecks,
  Megaphone,
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
}

export const NAV_GROUPS: NavGroup[] = [
  {
    label: "Overview",
    items: [
      {
        to: "/",
        label: "Overview",
        icon: Gauge,
        keywords: "overview health activity events status",
      },
      {
        to: "/inbox",
        label: "Decision Inbox",
        icon: Inbox,
        keywords: "decisions approvals blocked unresolved usage",
      },
    ],
  },
  {
    label: "Work",
    items: [
      {
        to: "/projects",
        label: "Projects",
        icon: FolderGit2,
        keywords: "repositories imports configuration",
      },
      {
        to: "/actions",
        label: "Actions",
        icon: ListChecks,
        keywords: "objectives specs checks risk",
      },
      {
        to: "/lead",
        label: "Lead",
        icon: Route,
        keywords: "assessment plan freeze authorize",
      },
      {
        to: "/delegations",
        label: "Delegations",
        icon: ShieldCheck,
        keywords: "scoped grants budgets revoke",
      },
      {
        to: "/campaigns",
        label: "Campaigns",
        icon: Megaphone,
        keywords: "runs attempts candidates gate quality",
      },
      {
        to: "/agents",
        label: "Agent Office",
        icon: Bot,
        keywords:
          "participants agents sessions implementer lead verifier reviewer worker tool events",
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
    ],
  },
  {
    label: "Operations",
    items: [
      {
        to: "/runtime",
        label: "Runtime",
        icon: Activity,
        keywords: "provider sessions model calls telemetry cleanup",
      },
      {
        to: "/settings",
        label: "Settings",
        icon: Settings,
        keywords: "providers models credentials runtime policy",
      },
    ],
  },
];

export const NAV_ITEMS = NAV_GROUPS.flatMap((group) => group.items);

export function routeLabel(pathname: string): string {
  if (pathname.startsWith("/campaigns/")) return "Campaign detail";
  if (pathname.startsWith("/actions/")) return "Action detail";
  if (pathname.startsWith("/agents/")) return "Agent Office participant";
  return (
    NAV_ITEMS.find((item) => item.to === pathname)?.label ?? "Factory Office"
  );
}
