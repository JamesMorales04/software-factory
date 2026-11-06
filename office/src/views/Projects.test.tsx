import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  GitHubRepositoryList,
  Project,
  ProjectImportOptions,
} from "../coreApi";
import { I18nProvider } from "../i18n";
import { Projects } from "./Projects";

const mocks = vi.hoisted(() => ({
  listProjects: vi.fn(),
  registerProject: vi.fn(),
  getProjectConfig: vi.fn(),
  updateProjectConfig: vi.fn(),
  getProjectImportOptions: vi.fn(),
  listGitHubRepositories: vi.fn(),
  importGitHubProject: vi.fn(),
}));

vi.mock("../coreApi", () => ({
  coreApi: mocks,
  CoreApiError: class CoreApiError extends Error {},
  newIdempotencyKey: () => "idempotency-key",
}));

const project: Project = {
  id: "project-1234567890",
  name: "Widgets",
  repository: "https://github.com/owner/repository.git",
  base_commit: "0123456789abcdef0123456789abcdef",
  created_at: "2026-10-27T10:00:00Z",
  source_kind: "github",
  source_url: "https://github.com/owner/repository",
  source_visibility: "private",
  default_branch: "main",
};

const disconnectedOptions: ProjectImportOptions = {
  github: {
    connected: false,
    enabled: false,
    login: null,
    scopes: [],
    scopes_reported: false,
    revision: 0,
    verified_at: null,
    revoked_at: null,
    detail: "GitHub is not connected",
    fine_grained_detail:
      "Fine-grained tokens do not report OAuth scopes. GitHub App and OAuth onboarding are not implemented.",
    app_oauth_available: false,
  },
  local: {
    enabled: true,
    managed_root: "/projects",
    detail:
      "Local import accepts only a Git repository whose canonical path is inside the operator-approved projects root.",
  },
  managed_storage_required: true,
};

const connectedOptions: ProjectImportOptions = {
  ...disconnectedOptions,
  github: {
    ...disconnectedOptions.github,
    connected: true,
    enabled: true,
    login: "acme-bot",
    scopes: [],
    scopes_reported: false,
    revision: 2,
    verified_at: "2026-10-28T10:00:00Z",
  },
};

const privateRepository: GitHubRepositoryList = {
  repositories: [
    {
      id: 42,
      full_name: "acme/private-widgets",
      private: true,
      default_branch: "develop",
      html_url: "https://github.com/acme/private-widgets",
      pushed_at: "2026-10-27T10:00:00Z",
      permissions: {
        admin: false,
        maintain: true,
        push: true,
        triage: true,
        pull: true,
      },
    },
  ],
  fetched_at: "2026-10-28T12:00:00Z",
  rate_limited: false,
  next_allowed_at: null,
  detail: "",
};

function renderProjects() {
  return render(
    <MemoryRouter>
      <I18nProvider initialLocale="es">
        <Projects />
      </I18nProvider>
    </MemoryRouter>,
  );
}

describe("Projects onboarding and list states", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.lang = "es";
    mocks.listProjects.mockReset().mockResolvedValue([project]);
    mocks.registerProject.mockReset();
    mocks.getProjectConfig.mockReset();
    mocks.updateProjectConfig.mockReset();
    mocks.getProjectImportOptions
      .mockReset()
      .mockResolvedValue(disconnectedOptions);
    mocks.listGitHubRepositories.mockReset().mockResolvedValue({
      repositories: [],
      fetched_at: "2026-10-28T12:00:00Z",
      rate_limited: false,
      next_allowed_at: null,
      detail: "",
    });
    mocks.importGitHubProject.mockReset();
  });

  it("renders the Projects workspace in Spanish with card-labelled cells", async () => {
    renderProjects();

    expect(
      await screen.findByRole("heading", { name: "Proyectos" }),
    ).toBeTruthy();
    expect(screen.getByText("Proyectos registrados (1)")).toBeTruthy();
    expect(screen.getByText("Widgets")).toBeTruthy();
    expect(screen.getByText("main")).toBeTruthy();
    expect(document.documentElement.lang).toBe("es");

    const table = screen.getByRole("table");
    expect(table.closest(".table-responsive")).not.toBeNull();
    const cells = table.querySelectorAll("tbody td");
    expect(cells).toHaveLength(5);
    for (const cell of cells) {
      expect(cell.getAttribute("data-label")).toBeTruthy();
    }
    expect(cells[0]?.getAttribute("data-label")).toBe("Proyecto");
    expect(cells[4]?.getAttribute("data-label")).toBe("Acciones");
    expect(screen.getByRole("button", { name: "Configurar" })).toBeTruthy();
  });

  it("renders the translated empty state with a real onboarding action", async () => {
    mocks.listProjects.mockResolvedValue([]);
    renderProjects();

    expect(
      await screen.findByText("No hay proyectos registrados."),
    ).toBeTruthy();
    expect(screen.getByText("Proyectos registrados (0)")).toBeTruthy();
    expect(screen.queryByRole("table")).toBeNull();
    expect(
      screen.getByRole("button", { name: "Conectar GitHub" }),
    ).toBeTruthy();
  });

  it("shows a truthful disconnected GitHub state without inventing a token UI", async () => {
    renderProjects();

    expect(await screen.findByText("GitHub no está conectado")).toBeTruthy();
    expect(
      screen.getByRole("link", { name: "Abrir integración con GitHub" }),
    ).toBeTruthy();
    expect(
      screen.getByText(
        /Configura un token cifrado en la integración con GitHub/,
      ),
    ).toBeTruthy();
    expect(mocks.listGitHubRepositories).not.toHaveBeenCalled();
  });

  it("lists authorized private repositories and imports the selected one", async () => {
    mocks.getProjectImportOptions.mockResolvedValue(connectedOptions);
    mocks.listGitHubRepositories.mockResolvedValue(privateRepository);
    mocks.importGitHubProject.mockResolvedValue({
      project,
      repository: privateRepository.repositories[0],
      binding: null,
      binding_error: null,
      sync_state: null,
      connection: connectedOptions.github,
    });
    const user = userEvent.setup();
    renderProjects();

    expect(await screen.findByText("acme/private-widgets")).toBeTruthy();
    expect(screen.getByText("privado")).toBeTruthy();
    expect(screen.getByText("push access")).toBeTruthy();
    await user.click(
      screen.getByRole("button", { name: "Importar repositorio" }),
    );
    await waitFor(() =>
      expect(mocks.importGitHubProject).toHaveBeenCalledWith(
        expect.objectContaining({
          owner: "acme",
          repo: "private-widgets",
          default_branch: "develop",
          idempotency_key: "idempotency-key",
        }),
      ),
    );
  });

  it("imports a local repository only with its explicit path and name", async () => {
    mocks.registerProject.mockResolvedValue(project);
    const user = userEvent.setup();
    renderProjects();

    await screen.findByText("Proyectos registrados (1)");
    await user.click(screen.getByRole("tab", { name: "Repositorio local" }));
    await user.type(
      screen.getByTestId("project-local-path"),
      "/projects/checkout",
    );
    await user.type(
      screen.getByLabelText("Nombre del proyecto (opcional)"),
      "Checkout",
    );
    await user.click(
      screen.getByRole("button", { name: "Importar repositorio local" }),
    );
    await waitFor(() =>
      expect(mocks.registerProject).toHaveBeenCalledWith({
        repository: "/projects/checkout",
        name: "Checkout",
        idempotency_key: "idempotency-key",
      }),
    );
  });

  it("shows a Core error instead of a fabricated success", async () => {
    mocks.listProjects.mockRejectedValue(new Error("Core is offline"));
    renderProjects();

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("No se pudieron cargar los proyectos.");
  });
});
