import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, useLocation } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider, LOCALE_STORAGE_KEY } from "../i18n";
import { ThemeProvider } from "../theme";
import { AppShell } from "./AppShell";

vi.mock("../coreApi", () => ({
  coreApi: {
    health: vi.fn().mockResolvedValue({ healthy: true, version: "test" }),
  },
}));

function LocationProbe() {
  const location = useLocation();
  return (
    <span data-testid="location">
      {location.pathname}
      {location.search}
    </span>
  );
}

function renderShell(pathname = "/projects", search = "") {
  return render(
    <I18nProvider initialLocale="en">
      <ThemeProvider>
        <MemoryRouter initialEntries={[`${pathname}${search}`]}>
          <AppShell onLock={() => undefined}>
            <LocationProbe />
          </AppShell>
        </MemoryRouter>
      </ThemeProvider>
    </I18nProvider>,
  );
}

describe("AppShell", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.dataset.theme = "system";
    document.documentElement.dataset.density = "comfortable";
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("keeps grouped navigation named, skip-link safe and main focusable", async () => {
    renderShell();
    const projects = screen.getByRole("link", { name: "Projects" });
    expect(projects.getAttribute("aria-label")).toBe("Projects");
    expect(screen.getByText("Advanced")).toBeDefined();
    await userEvent.click(
      screen.getByRole("button", { name: "Skip to main content" }),
    );
    expect(document.activeElement?.id).toBe("main");
    expect(screen.getByTestId("location").textContent).toBe("/projects");
  });

  it("marks only the current route active", () => {
    renderShell("/inbox");
    expect(
      screen
        .getByRole("link", { name: "Decisions" })
        .getAttribute("aria-current"),
    ).toBe("page");
    expect(
      screen.getByRole("link", { name: "Home" }).getAttribute("aria-current"),
    ).toBeNull();
    expect(
      screen
        .getByRole("link", { name: "Overview" })
        .getAttribute("aria-current"),
    ).toBeNull();
  });

  it("preserves every existing deep link in primary and advanced areas", () => {
    renderShell("/projects");
    const destinations: Array<[string, string]> = [
      ["Home", "/"],
      ["Projects", "/projects"],
      ["Campaigns", "/campaigns"],
      ["Decisions", "/inbox"],
      ["Agents", "/agents"],
      ["Settings", "/settings"],
      ["Overview", "/overview"],
      ["Actions", "/actions"],
      ["Lead", "/lead"],
      ["Delegations", "/delegations"],
      ["Runs", "/runs"],
      ["Workers", "/workers"],
      ["Runtime", "/runtime"],
      ["Access & Devices", "/settings?tab=access"],
    ];
    for (const [name, href] of destinations) {
      expect(screen.getByRole("link", { name }).getAttribute("href")).toBe(
        href,
      );
    }
  });

  it("exposes the advanced area as a named disclosure", async () => {
    renderShell("/projects");
    const toggle = screen.getByRole("button", { name: "Advanced" });
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByRole("link", { name: "Overview" })).toBeDefined();
    await userEvent.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByRole("link", { name: "Overview" })).toBeNull();
    await userEvent.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByRole("link", { name: "Overview" })).toBeDefined();
  });

  it("labels request workspaces, access routes and their owning section", () => {
    renderShell("/requests/request-1");
    expect(screen.getByText("Request")).toBeDefined();
    expect(
      screen.getByRole("link", { name: "Home" }).getAttribute("aria-current"),
    ).toBe("page");
  });

  it("distinguishes Settings from the query-backed Access & Devices route", () => {
    renderShell("/settings", "?tab=access");
    expect(document.querySelector(".topbar-title")?.textContent).toBe(
      "Access & Devices",
    );
    expect(
      screen
        .getByRole("link", { name: "Access & Devices" })
        .getAttribute("aria-current"),
    ).toBe("page");
    expect(
      screen
        .getByRole("link", { name: "Settings" })
        .getAttribute("aria-current"),
    ).toBeNull();
  });

  it("can collapse and expand the desktop sidebar without losing names", async () => {
    renderShell();
    await userEvent.click(
      screen.getByRole("button", { name: "Collapse navigation" }),
    );
    expect(
      screen.getByRole("button", { name: "Expand navigation" }),
    ).toBeDefined();
    expect(
      screen.getByRole("link", { name: "Projects" }).getAttribute("aria-label"),
    ).toBe("Projects");
    await userEvent.click(
      screen.getByRole("button", { name: "Expand navigation" }),
    );
    expect(
      screen.getByRole("button", { name: "Collapse navigation" }),
    ).toBeDefined();
  });

  it("recovers a collapsed shell from the visible topbar toggle", async () => {
    renderShell();
    const toggle = screen.getByRole("button", { name: "Toggle navigation" });
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    await userEvent.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(
      screen.getByRole("link", { name: "Projects" }).getAttribute("aria-label"),
    ).toBe("Projects");
    await userEvent.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
  });

  it("opens and closes the mobile drawer with correct state and focus", async () => {
    vi.stubGlobal("matchMedia", (query: string) => ({
      matches: true,
      media: query,
      onchange: null,
      addEventListener: () => undefined,
      removeEventListener: () => undefined,
      addListener: () => undefined,
      removeListener: () => undefined,
      dispatchEvent: () => false,
    }));
    renderShell();
    const toggle = screen.getByRole("button", { name: "Toggle navigation" });
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    await userEvent.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    await waitFor(() =>
      expect(document.activeElement).toBe(
        screen.getByRole("link", { name: "Home" }),
      ),
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Close navigation" }),
    );
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    await waitFor(() => expect(document.activeElement).toBe(toggle));
  });

  it("keeps every destination reachable in the narrow drawer", async () => {
    vi.stubGlobal("matchMedia", (query: string) => ({
      matches: true,
      media: query,
      onchange: null,
      addEventListener: () => undefined,
      removeEventListener: () => undefined,
      addListener: () => undefined,
      removeListener: () => undefined,
      dispatchEvent: () => false,
    }));
    renderShell("/settings");
    await userEvent.click(
      screen.getByRole("button", { name: "Toggle navigation" }),
    );
    for (const name of [
      "Home",
      "Projects",
      "Campaigns",
      "Decisions",
      "Agents",
      "Settings",
      "Overview",
      "Actions",
      "Lead",
      "Delegations",
      "Runs",
      "Workers",
      "Runtime",
      "Access & Devices",
    ]) {
      expect(screen.getByRole("link", { name })).toBeDefined();
    }
    await userEvent.click(
      screen.getByRole("button", { name: "Close navigation drawer" }),
    );
    await waitFor(() =>
      expect(document.activeElement).toBe(
        screen.getByRole("button", { name: "Toggle navigation" }),
      ),
    );
  });

  it("closes an open mobile drawer when the viewport becomes desktop", async () => {
    let mobile = true;
    const listeners = new Set<() => void>();
    vi.stubGlobal("matchMedia", (query: string) => ({
      get matches() {
        return mobile;
      },
      media: query,
      onchange: null,
      addEventListener: (_event: string, listener: () => void) => {
        listeners.add(listener);
      },
      removeEventListener: (_event: string, listener: () => void) => {
        listeners.delete(listener);
      },
      addListener: (listener: () => void) => listeners.add(listener),
      removeListener: (listener: () => void) => listeners.delete(listener),
      dispatchEvent: () => false,
    }));
    renderShell();
    await userEvent.click(
      screen.getByRole("button", { name: "Toggle navigation" }),
    );
    expect(document.querySelector(".app-shell--mobile-open")).not.toBeNull();
    mobile = false;
    act(() => {
      for (const listener of listeners) listener();
    });
    await waitFor(() =>
      expect(document.querySelector(".app-shell--mobile-open")).toBeNull(),
    );
    expect(document.querySelector(".workspace")?.hasAttribute("inert")).toBe(
      false,
    );
  });

  it("switches language from the global header without losing route context", async () => {
    const user = userEvent.setup();
    renderShell("/campaigns", "?status=BLOCKED_AUTHORIZATION");
    const select = screen.getByLabelText("Display language");

    await user.selectOptions(select, "es");

    expect(await screen.findByRole("link", { name: "Campañas" })).toBeDefined();
    expect(document.querySelector(".topbar-title")?.textContent).toBe(
      "Campañas",
    );
    expect(screen.getByTestId("location").textContent).toBe(
      "/campaigns?status=BLOCKED_AUTHORIZATION",
    );
    await waitFor(() => expect(document.documentElement.lang).toBe("es"));
    expect(localStorage.getItem(LOCALE_STORAGE_KEY)).toBe("es");
    expect(screen.getByRole("link", { name: "Inicio" })).toBeDefined();
  });

  it("opens command navigation with the keyboard and selects a page", async () => {
    renderShell("/");
    await userEvent.keyboard("{Control>}k{/Control}");
    const search = screen.getByPlaceholderText("Search implemented pages…");
    await userEvent.type(search, "projects");
    await userEvent.keyboard("{Enter}");
    expect(screen.getByTestId("location").textContent).toBe("/projects");
  });
});
