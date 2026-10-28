import { render, screen } from "@testing-library/react";
import type { ReactElement } from "react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { ActionDetail } from "../views/ActionDetail";
import { Actions } from "../views/Actions";
import { Campaigns } from "../views/Campaigns";
import { Lead } from "../views/Lead";

vi.mock("../coreApi", () => {
  class CoreApiError extends Error {
    readonly status = 500;
    readonly code = null;
    readonly reason = null;
    readonly body = null;
  }
  return {
    CoreApiError,
    newIdempotencyKey: () => "idem-test",
    coreApi: {
      listActions: vi.fn().mockResolvedValue([]),
      listProjects: vi.fn().mockResolvedValue([]),
      listProviders: vi
        .fn()
        .mockResolvedValue({ providers: [], supportedRoutes: [] }),
      listCampaigns: vi.fn().mockResolvedValue([]),
      listLeadRequests: vi.fn().mockResolvedValue([]),
      listDelegations: vi.fn().mockResolvedValue([]),
      listCandidates: vi.fn().mockResolvedValue([]),
      listAttempts: vi.fn().mockResolvedValue([]),
      listRuns: vi.fn().mockResolvedValue([]),
      getGate: vi.fn().mockResolvedValue(null),
      getProjectConfig: vi.fn().mockResolvedValue({
        configHash: "config-hash",
      }),
    },
  };
});

function renderRoutes(
  entry: string,
  path: string,
  element: ReactElement,
  locale: "en" | "es",
) {
  return render(
    <I18nProvider initialLocale={locale}>
      <MemoryRouter initialEntries={[entry]}>
        <Routes>
          <Route path={path} element={element} />
        </Routes>
      </MemoryRouter>
    </I18nProvider>,
  );
}

describe("localized operational views", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("renders Actions in English and Spanish without translating machine IDs", async () => {
    const english = renderRoutes("/actions", "/actions", <Actions />, "en");
    expect(
      await screen.findByRole("heading", { name: "Actions" }),
    ).toBeDefined();
    expect(
      screen.getByText(/Register a Project before creating an Action/),
    ).toBeDefined();
    english.unmount();

    renderRoutes("/actions", "/actions", <Actions />, "es");
    expect(
      await screen.findByRole("heading", { name: "Acciones" }),
    ).toBeDefined();
    expect(screen.getByText("Todas las Acciones (0)")).toBeDefined();
    expect(
      screen.getByText(/Registra un Proyecto antes de crear una Acción/),
    ).toBeDefined();
  });

  it("renders Action detail not-found states in both locales", async () => {
    const english = renderRoutes(
      "/actions/action-1",
      "/actions/:actionId",
      <ActionDetail />,
      "en",
    );
    expect(await screen.findByText("Action not found")).toBeDefined();
    expect(
      screen.getByText("No Action action-1 is recorded in Core."),
    ).toBeDefined();
    english.unmount();

    renderRoutes(
      "/actions/action-1",
      "/actions/:actionId",
      <ActionDetail />,
      "es",
    );
    expect(await screen.findByText("Acción no encontrada")).toBeDefined();
    expect(
      screen.getByText("No hay ninguna Acción action-1 registrada en Core."),
    ).toBeDefined();
    expect(
      screen.getByRole("link", { name: "Volver a Acciones" }),
    ).toBeDefined();
  });

  it("renders Campaigns and Lead in both locales", async () => {
    const englishCampaigns = renderRoutes(
      "/campaigns",
      "/campaigns",
      <Campaigns />,
      "en",
    );
    expect(
      await screen.findByRole("heading", { name: "Campaigns" }),
    ).toBeDefined();
    expect(
      screen.getByText(/Create an Action before starting a Campaign/),
    ).toBeDefined();
    englishCampaigns.unmount();

    const spanishCampaigns = renderRoutes(
      "/campaigns",
      "/campaigns",
      <Campaigns />,
      "es",
    );
    expect(
      await screen.findByRole("heading", { name: "Campañas" }),
    ).toBeDefined();
    expect(
      screen.getByText(/Crea una Acción antes de iniciar una Campaña/),
    ).toBeDefined();
    spanishCampaigns.unmount();

    renderRoutes("/lead", "/lead", <Lead />, "es");
    expect(await screen.findByRole("heading", { name: "Lead" })).toBeDefined();
    expect(screen.getByText("Enviar una solicitud de Lead")).toBeDefined();
    expect(screen.getByText("No hay solicitudes de Lead.")).toBeDefined();
  });
});
