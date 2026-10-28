import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Project } from "../coreApi";
import { I18nProvider } from "../i18n";
import { Projects } from "./Projects";

const mocks = vi.hoisted(() => ({
  listProjects: vi.fn(),
  registerProject: vi.fn(),
  getProjectConfig: vi.fn(),
  updateProjectConfig: vi.fn(),
}));

vi.mock("../coreApi", () => ({
  coreApi: mocks,
  CoreApiError: class CoreApiError extends Error {},
  newIdempotencyKey: () => "idempotency-key",
}));

const project: Project = {
  id: "project-1234567890",
  repository: "https://github.com/owner/repository.git",
  base_commit: "0123456789abcdef0123456789abcdef",
  created_at: "2026-10-27T10:00:00Z",
};

describe("Projects localization and responsive table semantics", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.lang = "en";
    mocks.listProjects.mockReset().mockResolvedValue([project]);
  });

  it("renders the Projects workspace in Spanish with card-labelled cells", async () => {
    render(
      <I18nProvider initialLocale="es">
        <Projects />
      </I18nProvider>,
    );

    expect(
      await screen.findByRole("heading", { name: "Proyectos" }),
    ).toBeTruthy();
    expect(screen.getByText("Proyectos registrados (1)")).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Registrar proyecto" }),
    ).toBeTruthy();
    expect(screen.getByRole("button", { name: "Configurar" })).toBeTruthy();
    expect(document.documentElement.lang).toBe("es");

    const table = screen.getByRole("table");
    expect(table.closest(".table-responsive")).not.toBeNull();
    const cells = table.querySelectorAll("tbody td");
    expect(cells).toHaveLength(5);
    for (const cell of cells) {
      expect(cell.getAttribute("data-label")).toBeTruthy();
    }
    expect(cells[1]?.getAttribute("data-label")).toBe("Repositorio");
    expect(cells[4]?.getAttribute("data-label")).toBe("Acciones");
    expect(cells[4]?.querySelector("button")?.hasAttribute("hidden")).toBe(
      false,
    );
  });

  it("renders the translated empty state", async () => {
    mocks.listProjects.mockResolvedValue([]);
    render(
      <I18nProvider initialLocale="es">
        <Projects />
      </I18nProvider>,
    );

    expect(
      await screen.findByText("No hay proyectos registrados."),
    ).toBeTruthy();
    expect(screen.getByText("Proyectos registrados (0)")).toBeTruthy();
    expect(screen.queryByRole("table")).toBeNull();
  });
});
