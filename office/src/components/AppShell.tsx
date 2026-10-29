import {
  ChevronDown,
  ChevronRight,
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
  X,
} from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { Link, useLocation } from "react-router-dom";
import { LanguageSwitcher, type MessageKey, useI18n } from "../i18n";
import {
  isAdvancedRoute,
  isNavItemActive,
  NAV_GROUPS,
  type NavItem,
  routeLabel,
} from "../navigation";
import { type Density, type Theme, useTheme } from "../theme";
import { CommandPalette } from "./CommandPalette";
import { HealthBadge } from "./HealthBadge";
import { IconButton } from "./ui/Button";
import { Tooltip } from "./ui/Tooltip";

const SIDEBAR_KEY = "factory-office-sidebar";

const THEME_KEY: Record<Theme, MessageKey> = {
  system: "shell.theme.system",
  light: "shell.theme.light",
  dark: "shell.theme.dark",
};

const DENSITY_KEY: Record<Density, MessageKey> = {
  comfortable: "shell.density.comfortable",
  compact: "shell.density.compact",
};

function isMobileViewport(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(max-width: 1000px)").matches
  );
}

function nextTheme(theme: Theme): Theme {
  if (theme === "system") return "light";
  if (theme === "light") return "dark";
  return "system";
}

function NavLinkItem({
  item,
  label,
  pathname,
  search,
  onNavigate,
}: {
  item: NavItem;
  label: string;
  pathname: string;
  search: string;
  onNavigate: () => void;
}) {
  const Icon = item.icon;
  const active = isNavItemActive(item, pathname, search);
  return (
    <Link
      to={item.to}
      className={`nav-link ${active ? "nav-link--active" : ""}`}
      aria-current={active ? "page" : undefined}
      aria-label={label}
      title={label}
      onClick={onNavigate}
    >
      <Icon size={18} aria-hidden="true" />
      <span>{label}</span>
    </Link>
  );
}

export function AppShell({
  children,
  onLock,
}: {
  children: ReactNode;
  onLock: () => void;
}) {
  const { theme, setTheme, density, setDensity } = useTheme();
  const { t } = useI18n();
  const menuRef = useRef<HTMLButtonElement>(null);
  const sidebarRef = useRef<HTMLElement>(null);
  const [collapsed, setCollapsed] = useState(
    () => localStorage.getItem(SIDEBAR_KEY) === "collapsed",
  );
  const [mobileOpen, setMobileOpen] = useState(false);
  const [isMobile, setIsMobile] = useState(isMobileViewport);
  const [advancedOpen, setAdvancedOpen] = useState(true);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const location = useLocation();
  const pathname = location.pathname;
  const search = location.search;
  const studioView =
    pathname.startsWith("/agents") &&
    (new URLSearchParams(search).get("view") ?? "floor") === "floor";
  const advancedActive = isAdvancedRoute(pathname, search);

  useEffect(() => {
    localStorage.setItem(SIDEBAR_KEY, collapsed ? "collapsed" : "expanded");
  }, [collapsed]);

  useEffect(() => {
    if (advancedActive) setAdvancedOpen(true);
  }, [advancedActive]);

  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const media = window.matchMedia("(max-width: 1000px)");
    const update = () => {
      setIsMobile(media.matches);
      if (!media.matches) setMobileOpen(false);
    };
    update();
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);

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

  useEffect(() => {
    if (!mobileOpen) return;
    const first =
      sidebarRef.current?.querySelector<HTMLAnchorElement>(
        ".sidebar .nav-link",
      );
    first?.focus();
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setMobileOpen(false);
        requestAnimationFrame(() => menuRef.current?.focus());
        return;
      }
      if (event.key !== "Tab") return;
      const focusable = Array.from(
        sidebarRef.current?.querySelectorAll<HTMLElement>(
          'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
        ) ?? [],
      ).filter((element) => element.closest("[hidden]") === null);
      if (focusable.length === 0) return;
      const firstFocusable = focusable[0];
      const lastFocusable = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === firstFocusable) {
        event.preventDefault();
        lastFocusable.focus();
      } else if (!event.shiftKey && document.activeElement === lastFocusable) {
        event.preventDefault();
        firstFocusable.focus();
      }
    }
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [mobileOpen]);

  function closeMobileDrawer() {
    setMobileOpen(false);
    requestAnimationFrame(() => menuRef.current?.focus());
  }

  const ThemeIcon = theme === "light" ? Sun : theme === "dark" ? Moon : Monitor;
  const DensityIcon = density === "compact" ? Rows4 : Rows3;

  return (
    <>
      <button
        type="button"
        className="skip-link"
        onClick={() => document.getElementById("main")?.focus()}
      >
        {t("shell.skipToMain")}
      </button>
      <div
        className={`app-shell ${collapsed ? "app-shell--collapsed" : ""} ${
          mobileOpen ? "app-shell--mobile-open" : ""
        } ${studioView ? "app-shell--studio" : ""}`}
      >
        <aside
          ref={sidebarRef}
          className="sidebar"
          id="primary-sidebar"
          aria-label={t("shell.primaryNavigation")}
        >
          <div className="sidebar-brand">
            <span className="brand-mark" aria-hidden="true">
              SF
            </span>
            <span className="brand-copy">
              <strong>Factory</strong>
              <span>Office</span>
            </span>
            {isMobile ? (
              <IconButton
                label={t("shell.closeDrawer")}
                className="sidebar-close"
                onClick={closeMobileDrawer}
              >
                <X size={17} aria-hidden="true" />
              </IconButton>
            ) : (
              <IconButton
                label={
                  collapsed
                    ? t("shell.expandNavigation")
                    : t("shell.collapseNavigation")
                }
                className="sidebar-collapse"
                onClick={() => setCollapsed((value) => !value)}
              >
                {collapsed ? (
                  <PanelLeftOpen size={17} aria-hidden="true" />
                ) : (
                  <PanelLeftClose size={17} aria-hidden="true" />
                )}
              </IconButton>
            )}
          </div>
          <nav className="sidebar-nav" aria-label={t("shell.navigation")}>
            {NAV_GROUPS.map((group) => {
              const collapsible = group.collapsible === true;
              const groupLabel = t(group.labelKey);
              return (
                <div className="nav-group" key={group.labelKey}>
                  {collapsible ? (
                    <>
                      <button
                        type="button"
                        className="nav-group-toggle"
                        aria-expanded={advancedOpen}
                        aria-controls="nav-group-advanced"
                        onClick={() => setAdvancedOpen((open) => !open)}
                      >
                        <span>{groupLabel}</span>
                        {advancedOpen ? (
                          <ChevronDown size={14} aria-hidden="true" />
                        ) : (
                          <ChevronRight size={14} aria-hidden="true" />
                        )}
                      </button>
                      <div
                        className="nav-group-items"
                        id="nav-group-advanced"
                        hidden={!advancedOpen}
                      >
                        {group.items.map((item) => (
                          <NavLinkItem
                            key={item.to}
                            item={item}
                            label={t(item.labelKey)}
                            pathname={pathname}
                            search={search}
                            onNavigate={() => setMobileOpen(false)}
                          />
                        ))}
                      </div>
                    </>
                  ) : (
                    <>
                      <p className="nav-group-label">{groupLabel}</p>
                      <div className="nav-group-items">
                        {group.items.map((item) => (
                          <NavLinkItem
                            key={item.to}
                            item={item}
                            label={t(item.labelKey)}
                            pathname={pathname}
                            search={search}
                            onNavigate={() => setMobileOpen(false)}
                          />
                        ))}
                      </div>
                    </>
                  )}
                </div>
              );
            })}
          </nav>
          <div className="sidebar-footer" aria-hidden="true" />
        </aside>
        {mobileOpen && (
          <button
            type="button"
            className="sidebar-scrim"
            aria-label={t("shell.closeNavigation")}
            onClick={closeMobileDrawer}
          />
        )}
        <div className="workspace" inert={mobileOpen || undefined}>
          <header className="topbar">
            <IconButton
              ref={menuRef}
              label={t("shell.toggleNavigation")}
              className="topbar-menu"
              aria-controls="primary-sidebar"
              aria-expanded={isMobile ? mobileOpen : !collapsed}
              onClick={() => {
                if (isMobile) {
                  setMobileOpen((open) => !open);
                } else {
                  setCollapsed((value) => !value);
                }
              }}
            >
              {(isMobile ? mobileOpen : !collapsed) ? (
                <PanelLeftClose size={19} aria-hidden="true" />
              ) : (
                <PanelLeftOpen size={19} aria-hidden="true" />
              )}
            </IconButton>
            <div className="topbar-context">
              <span className="topbar-crumb">Factory Office</span>
              <span className="topbar-separator" aria-hidden="true">
                /
              </span>
              <span className="topbar-title">
                {routeLabel(pathname, search, t)}
              </span>
            </div>
            <div className="topbar-actions">
              <HealthBadge />
              <button
                type="button"
                className="command-trigger"
                onClick={() => setPaletteOpen(true)}
                aria-label={t("shell.openCommandNavigation")}
              >
                <Search size={16} aria-hidden="true" />
                <span>{t("shell.search")}</span>
                <kbd className="ui-kbd">
                  <Command size={11} aria-hidden="true" />K
                </kbd>
              </button>
              <LanguageSwitcher variant="compact" />
              <Tooltip
                label={t("shell.theme.switch", { theme: t(THEME_KEY[theme]) })}
              >
                <IconButton
                  label={t("shell.theme.switchAria", {
                    theme: t(THEME_KEY[theme]),
                  })}
                  className="topbar-preference"
                  onClick={() => setTheme(nextTheme(theme))}
                >
                  <ThemeIcon size={17} aria-hidden="true" />
                </IconButton>
              </Tooltip>
              <Tooltip
                label={t("shell.density.switch", {
                  density: t(DENSITY_KEY[density]),
                })}
              >
                <IconButton
                  label={t("shell.density.switchAria", {
                    density: t(DENSITY_KEY[density]),
                  })}
                  className="topbar-preference"
                  onClick={() =>
                    setDensity(
                      density === "comfortable" ? "compact" : "comfortable",
                    )
                  }
                >
                  <DensityIcon size={17} aria-hidden="true" />
                </IconButton>
              </Tooltip>
              <Tooltip label={t("shell.lock")}>
                <IconButton label={t("shell.lock")} onClick={onLock}>
                  <Lock size={17} aria-hidden="true" />
                </IconButton>
              </Tooltip>
            </div>
          </header>
          <main id="main" className="workspace-main" tabIndex={-1}>
            <h1 className="ui-visually-hidden">{t("shell.mainHeading")}</h1>
            {children}
          </main>
        </div>
      </div>
      <CommandPalette open={paletteOpen} onOpenChange={setPaletteOpen} />
    </>
  );
}
