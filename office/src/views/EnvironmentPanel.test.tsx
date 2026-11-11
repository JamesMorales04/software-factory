import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { EnvironmentPlanView, EnvironmentStateView } from "../coreApi";
import { I18nProvider } from "../i18n";
import { EnvironmentPanel } from "./EnvironmentPanel";

const mocks = vi.hoisted(() => ({
  reviewAgenticEnvironment: vi.fn(),
}));

vi.mock("../coreApi", () => ({
  coreApi: mocks,
  CoreApiError: class CoreApiError extends Error {
    readonly status: number;
    constructor(status: number, message: string) {
      super(message);
      this.name = "CoreApiError";
      this.status = status;
    }
  },
}));

const plan: EnvironmentPlanView = {
  schemaVersion: 1,
  planId: "plan-1",
  projectId: "project-1",
  requestId: "request-1",
  actionId: "action-1",
  campaignId: null,
  baseCommit: "1111111111111111111111111111111111111111",
  conventionsSha256: "2".repeat(64),
  riskClass: "R0",
  conventions: {
    schemaVersion: 1,
    projectId: "project-1",
    baseCommit: "1111111111111111111111111111111111111111",
    conventionsSha256: "2".repeat(64),
    scannedAt: "2026-11-10T12:00:00Z",
    files: [
      {
        path: "devenv.lock",
        kind: "DEVENV_LOCK",
        sha256: "3".repeat(64),
        precedence: "COMMITTED_ENVIRONMENT",
        pins: [
          {
            name: "nixpkgs",
            value:
              "github:NixOS/nixpkgs/1111111111111111111111111111111111111111",
            source: "devenv.lock",
          },
        ],
      },
    ],
    commands: [],
    conflicts: [],
    truncated: false,
    skippedEntries: 0,
    provenance: "git-object-store",
    existingEnvironment: "DEVENV",
  },
  status: "CORE_VALIDATED",
  revision: 1,
  source: "PROJECT_COMMITTED",
  toolchains: [
    {
      id: "rust",
      version: "1.90.0",
      source: "rust-toolchain.toml",
      evidence: [],
    },
  ],
  packages: [
    {
      name: "ripgrep",
      version: "14.1.1",
      license: "MIT",
      source: "nixpkgs",
      attr: "ripgrep",
      sha256: "4".repeat(64),
      unfree: false,
      attribution: "MIT ripgrep",
    },
  ],
  services: [],
  commands: {},
  browsers: [],
  files: [
    {
      path: "devenv.lock",
      sha256: "3".repeat(64),
      origin: "PROJECT",
    },
  ],
  nix: {
    nixpkgs: "github:NixOS/nixpkgs/1111111111111111111111111111111111111111",
    inputs: [],
    allowUnfree: false,
    sandbox: {
      sandbox: true,
      impure: false,
      hostMounts: [],
      privileged: false,
      hostNetwork: false,
      hostSockets: [],
    },
  },
  resources: {
    cpu: 2,
    memoryMib: 4096,
    diskMib: 10240,
    pids: 512,
    timeoutSecs: 1800,
  },
  network: { build: "NONE", fetch: "BROKER", fetchAllowlist: [] },
  licenseInventory: {
    schemaVersion: 1,
    policyRevision: "nix-devenv-license-v1",
    allowUnfree: false,
    entries: [
      {
        name: "nixpkgs",
        version: "pinned",
        license: "MIT",
        source: "https://github.com/NixOS/nixpkgs",
        sha256: "",
        unfree: false,
        ownerOverrideId: null,
        attribution: "MIT",
      },
    ],
    overrides: [],
    blockers: [],
    inventorySha256: "5".repeat(64),
    sbomSha256: "6".repeat(64),
  },
  licenseInventorySha256: "5".repeat(64),
  builder: {
    image: "factory/environment-builder",
    digest: `sha256:${"7".repeat(64)}`,
    platform: "x86_64-linux",
    policyRevision: "nix-devenv-environment-v1",
    status: "READY",
  },
  leadReview: null,
  blockers: [],
  commandsRationale: "No command was committed.",
  planSha256: "8".repeat(64),
  createdAt: "2026-11-10T12:00:00Z",
};

function state(
  overrides: Partial<EnvironmentStateView> = {},
): EnvironmentStateView {
  return {
    required: true,
    status: "CORE_VALIDATED",
    plan,
    planId: plan.planId,
    planSha256: plan.planSha256,
    planRevision: plan.revision,
    conventions: plan.conventions,
    reviews: [],
    leadApproval: null,
    spec: null,
    specSha256: null,
    builder: {
      descriptor: plan.builder,
      receipt: null,
    },
    licenseInventory: plan.licenseInventory,
    blockers: [],
    ...overrides,
  };
}

function renderPanel(environment: EnvironmentStateView | null) {
  return render(
    <I18nProvider initialLocale="en">
      <EnvironmentPanel environment={environment} requestId="request-1" />
    </I18nProvider>,
  );
}

describe("EnvironmentPanel", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.lang = "en";
    mocks.reviewAgenticEnvironment.mockReset();
  });

  it("shows a truthful empty state when no plan exists", () => {
    renderPanel(null);
    expect(
      screen.getByRole("heading", { name: "No environment plan recorded" }),
    ).toBeTruthy();
    expect(screen.getByText("No environment plan")).toBeTruthy();
  });

  it("shows detection evidence, pins, license inventory and exact plan identity", () => {
    renderPanel(state());
    expect(screen.getByText("devenv.lock")).toBeTruthy();
    expect(screen.getByText(/nixpkgs=github:NixOS/)).toBeTruthy();
    expect(screen.getByText("MIT")).toBeTruthy();
    expect(screen.getByText(plan.planSha256)).toBeTruthy();
    expect(screen.getByText("Core validated")).toBeTruthy();
  });

  it("triggers the frozen Lead review without letting the operator choose a decision", async () => {
    const user = userEvent.setup();
    const approved = state({
      status: "FROZEN",
      leadApproval: {
        schemaVersion: 1,
        reviewId: "review-1",
        requestId: "request-1",
        actionId: "action-1",
        planId: plan.planId,
        planSha256: plan.planSha256,
        planRevision: 1,
        riskClass: "R0",
        reviewerRole: "LEAD",
        principalId: "lead:lead-request-1",
        recordedBy: "core:lead-environment-review",
        decision: "APPROVED",
        rationale: "The plan matches the committed conventions.",
        constraints: [],
        decidedAt: "2026-11-10T12:01:00Z",
        evidenceRefs: [],
        modelCall: {
          mechanism: "MODEL",
          provider: "opencode-go",
          model: "deepseek-v4.1-flash",
          reservationId: "reservation-1",
          requestSha256: "d".repeat(64),
          inputTokens: 10,
          outputTokens: 5,
          totalTokens: 15,
          costMicros: 1,
          usageKnown: true,
        },
      },
    });
    mocks.reviewAgenticEnvironment.mockResolvedValue(approved);
    renderPanel(state());
    const trigger = screen.getByRole("button", {
      name: "Request Lead environment review",
    });
    expect((trigger as HTMLButtonElement).disabled).toBe(false);
    await user.click(trigger);
    await waitFor(() =>
      expect(mocks.reviewAgenticEnvironment).toHaveBeenCalledWith("request-1", {
        planSha256: plan.planSha256,
        planRevision: 1,
      }),
    );
    expect(
      await screen.findByText("The Lead environment review completed."),
    ).toBeTruthy();
  });

  it("shows license blockers and unfree state without hiding the plan", () => {
    const blocked = state({
      status: "BLOCKED",
      plan: {
        ...plan,
        licenseInventory: {
          ...plan.licenseInventory,
          blockers: [
            {
              code: "ENVIRONMENT_LICENSE_BLOCKED",
              message:
                "proprietary-tool 1.0.0 is unfree and has no exact authenticated owner override",
            },
          ],
          entries: [
            ...plan.licenseInventory.entries,
            {
              name: "proprietary-tool",
              version: "1.0.0",
              license: "MIT",
              source: "nixpkgs",
              sha256: "9".repeat(64),
              unfree: true,
              ownerOverrideId: null,
              attribution: "Proprietary Tool",
            },
          ],
        },
      },
      licenseInventory: {
        ...plan.licenseInventory,
        blockers: [
          {
            code: "ENVIRONMENT_LICENSE_BLOCKED",
            message:
              "proprietary-tool 1.0.0 is unfree and has no exact authenticated owner override",
          },
        ],
      },
      blockers: [
        {
          code: "ENVIRONMENT_LICENSE_BLOCKED",
          message:
            "proprietary-tool 1.0.0 is unfree and has no exact authenticated owner override",
        },
      ],
    });
    renderPanel(blocked);
    expect(
      screen.getAllByText("ENVIRONMENT_LICENSE_BLOCKED").length,
    ).toBeGreaterThanOrEqual(2);
    expect(screen.getByText("proprietary-tool")).toBeTruthy();
    expect(screen.getAllByText("Yes").length).toBeGreaterThanOrEqual(1);
  });

  it("shows a typed missing-pin blocker and the constraints action", () => {
    renderPanel(
      state({
        status: "BLOCKED",
        plan: null,
        blockers: [
          {
            code: "ENVIRONMENT_PIN_MISSING",
            message:
              "the environment plan requires a committed or explicit pinned nixpkgs input",
          },
        ],
      } as Partial<EnvironmentStateView>),
    );
    expect(screen.getByText("ENVIRONMENT_PIN_MISSING")).toBeTruthy();
    expect(
      screen.getByText(/Add explicit environment constraints/),
    ).toBeTruthy();
  });

  it("shows builder unavailability and frozen spec evidence", () => {
    const frozen = state({
      status: "FROZEN",
      spec: {
        schemaVersion: 1,
        specId: "spec-1",
        specSha256: "a".repeat(64),
        projectId: "project-1",
        requestId: "request-1",
        actionId: "action-1",
        campaignId: null,
        baseCommit: plan.baseCommit,
        planId: plan.planId,
        planSha256: plan.planSha256,
        conventionsSha256: plan.conventionsSha256,
        reviewId: "review-1",
        platform: "x86_64-linux",
        nixVersion: "2.28.3",
        devenvVersion: "1.10.0",
        flakeLockSha256: "",
        devenvLockSha256: "b".repeat(64),
        licenseInventorySha256: "5".repeat(64),
        sbomSha256: "6".repeat(64),
        builderImage: "",
        builderImageDigest: "",
        resourcePolicy: plan.resources,
        networkPolicy: plan.network,
        status: "FROZEN",
        frozenAt: "2026-11-10T12:02:00Z",
      },
      specSha256: "a".repeat(64),
      builder: {
        descriptor: {
          ...plan.builder,
          image: "",
          digest: "",
          status: "UNAVAILABLE",
        },
        receipt: null,
      },
      blockers: [
        {
          code: "ENVIRONMENT_BUILDER_UNAVAILABLE",
          message: "the environment builder has no successful receipt",
        },
      ],
    });
    renderPanel(frozen);
    expect(screen.getByText("UNAVAILABLE")).toBeTruthy();
    expect(
      screen.getByText("No successful build receipt recorded"),
    ).toBeTruthy();
    expect(screen.getByText("a".repeat(64))).toBeTruthy();
    expect(screen.getByText(/A frozen spec is immutable/)).toBeTruthy();
  });
});
