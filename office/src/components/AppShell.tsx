import {
  Command,
  Lock,
  Monitor,
  Moon,
  PanelLeftClose,
  PanelLeftOpen,
  Rows3,
  Rows4,
  Search,
  Sun,
} from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { NavLink } from "react-router-dom";
import { NAV_GROUPS, routeLabel } from "../navigation";
import { type Theme, useTheme } from "../theme";
import { CommandPalette } from "./CommandPalette";
import { HealthBadge } from "./HealthBadge";
import { IconButton } from "./ui/Button";
import { Tooltip } from "./ui/Tooltip";

const SIDEBAR_KEY = "factory-office-sidebar";

const THEME_LABEL: Record<Theme, string> = {
  system: "System theme",
  light: "Light theme",
  dark: "Dark theme",
};

function nextTheme(theme: Theme): Theme {
  if (theme === "system") return "light";
  if (theme === "light") return "dark";
  return "system";
}

export function AppShell({
  children,
  onLock,
  pathname,
}: {
  children: ReactNode;
  onLock: () => void;
  pathname: string;
}) {
  const { theme, setTheme, density, setDensity } = useTheme();
  const [collapsed, setCollapsed] = useState(
    () => localStorage.getItem(SIDEBAR_KEY) === "collapsed",
  );
  const [mobileOpen, setMobileOpen] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);

  useEffect(() => {
    localStorage.setItem(SIDEBAR_KEY, collapsed ? "collapsed" : "expanded");
  }, [collapsed]);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setPaletteOpen((open) => !open);
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  const ThemeIcon = theme === "light" ? Sun : theme === "dark" ? Moon : Monitor;
  const DensityIcon = density === "compact" ? Rows4 : Rows3;

  return (
    <>
      <a className="skip-link" href="#main">
        Skip to main content
      </a>
      <div
        className={`app-shell ${collapsed ? "app-shell--collapsed" : ""} ${
          mobileOpen ? "app-shell--mobile-open" : ""
        }`}
      >
        <aside className="sidebar" aria-label="Primary navigation">
          <div className="sidebar-brand">
            <span className="brand-mark" aria-hidden="true">
              SF
            </span>
            <span className="brand-copy">
              <strong>Factory</strong>
              <span>Office</span>
            </span>
            <IconButton
              label={collapsed ? "Expand navigation" : "Collapse navigation"}
              className="sidebar-collapse"
              onClick={() => setCollapsed((value) => !value)}
            >
              {collapsed ? (
                <PanelLeftOpen size={17} aria-hidden="true" />
              ) : (
                <PanelLeftClose size={17} aria-hidden="true" />
              )}
            </IconButton>
          </div>
          <nav className="sidebar-nav">
            {NAV_GROUPS.map((group) => (
              <div className="nav-group" key={group.label}>
                <p className="nav-group-label">{group.label}</p>
                {group.items.map((item) => {
                  const Icon = item.icon;
                  return (
                    <NavLink
                      key={item.to}
                      to={item.to}
                      end={item.to === "/"}
                      className={({ isActive }) =>
                        `nav-link ${isActive ? "nav-link--active" : ""}`
                      }
                      onClick={() => setMobileOpen(false)}
                    >
                      <Icon size={18} aria-hidden="true" />
                      <span>{item.label}</span>
                    </NavLink>
                  );
                })}
              </div>
            ))}
          </nav>
          <div className="sidebar-footer">
            <HealthBadge />
          </div>
        </aside>
        {mobileOpen && (
          <button
            type="button"
            className="sidebar-scrim"
            aria-label="Close navigation"
            onClick={() => setMobileOpen(false)}
          />
        )}
        <div className="workspace">
          <header className="topbar">
            <IconButton
              label="Open navigation"
              className="topbar-menu"
              onClick={() => setMobileOpen(true)}
            >
              <PanelLeftOpen size={19} aria-hidden="true" />
            </IconButton>
            <div className="topbar-context">
              <span className="topbar-crumb">Factory Office</span>
              <span className="topbar-separator" aria-hidden="true">
                /
              </span>
              <span className="topbar-title">{routeLabel(pathname)}</span>
            </div>
            <div className="topbar-actions">
              <button
                type="button"
                className="command-trigger"
                onClick={() => setPaletteOpen(true)}
                aria-label="Open command navigation"
              >
                <Search size={16} aria-hidden="true" />
                <span>Search</span>
                <kbd className="ui-kbd">
                  <Command size={11} aria-hidden="true" />K
                </kbd>
              </button>
              <Tooltip label={`${THEME_LABEL[theme]} — switch`}>
                <IconButton
                  label={`${THEME_LABEL[theme]}; switch theme`}
                  onClick={() => setTheme(nextTheme(theme))}
                >
                  <ThemeIcon size={17} aria-hidden="true" />
                </IconButton>
              </Tooltip>
              <Tooltip label={`${density} density — switch`}>
                <IconButton
                  label={`${density} density; switch density`}
                  onClick={() =>
                    setDensity(
                      density === "comfortable" ? "compact" : "comfortable",
                    )
                  }
                >
                  <DensityIcon size={17} aria-hidden="true" />
                </IconButton>
              </Tooltip>
              <Tooltip label="Lock Office">
                <IconButton label="Lock Office" onClick={onLock}>
                  <Lock size={17} aria-hidden="true" />
                </IconButton>
              </Tooltip>
            </div>
          </header>
          <main id="main" className="workspace-main" tabIndex={-1}>
            <h1 className="ui-visually-hidden">
              Factory Office command center
            </h1>
            {children}
          </main>
        </div>
      </div>
      <CommandPalette open={paletteOpen} onOpenChange={setPaletteOpen} />
    </>
  );
}
