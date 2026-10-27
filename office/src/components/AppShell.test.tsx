import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, useLocation } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ThemeProvider } from "../theme";
import { AppShell } from "./AppShell";

vi.mock("../coreApi", () => ({
  coreApi: {
    health: vi.fn().mockResolvedValue({ healthy: true, version: "test" }),
  },
}));

function LocationProbe() {
  const location = useLocation();
  return <span data-testid="location">{location.pathname}</span>;
}

function renderShell(pathname = "/projects") {
  return render(
    <ThemeProvider>
      <MemoryRouter initialEntries={[pathname]}>
        <AppShell pathname={pathname} onLock={() => undefined}>
          <LocationProbe />
        </AppShell>
      </MemoryRouter>
    </ThemeProvider>,
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
    expect(screen.getByText("Operations")).toBeDefined();
    await userEvent.click(
      screen.getByRole("button", { name: "Skip to main content" }),
    );
    expect(document.activeElement?.id).toBe("main");
    expect(screen.getByTestId("location").textContent).toBe("/projects");
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
        screen.getByRole("link", { name: "Dashboard" }),
      ),
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Close navigation" }),
    );
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    await waitFor(() => expect(document.activeElement).toBe(toggle));
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
