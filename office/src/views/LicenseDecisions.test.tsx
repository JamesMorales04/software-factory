import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { LicenseDecisions } from "./LicenseDecisions";

const mocks = vi.hoisted(() => ({
  listProjects: vi.fn(),
  listAgenticDecisions: vi.fn(),
  listLicenseDecisions: vi.fn(),
  createAgenticOwnerDecision: vi.fn(),
  resolveAgenticOwnerDecision: vi.fn(),
  revokeLicenseDecision: vi.fn(),
  revokeLicenseDelegation: vi.fn(),
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

function renderView() {
  return render(
    <I18nProvider initialLocale="en">
      <LicenseDecisions />
    </I18nProvider>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.listProjects.mockResolvedValue([
    {
      id: "project-1",
      repository: "/repo",
      base_commit: "b".repeat(40),
      created_at: "2026-11-20T00:00:00Z",
      name: "Project One",
    },
  ]);
  mocks.listAgenticDecisions.mockResolvedValue({ decisions: [] });
  mocks.listLicenseDecisions.mockResolvedValue({ decisions: [] });
});

describe("LicenseDecisions", () => {
  it("renders the exact delegation and decision empty states", async () => {
    renderView();
    expect(
      await screen.findByText("Licenses, identity and evidence"),
    ).toBeTruthy();
    expect(screen.getByText("No licence delegation")).toBeTruthy();
    expect(screen.getByText("No Lead decisions")).toBeTruthy();
  });

  it("creates an exact Owner delegation through the real action", async () => {
    mocks.createAgenticOwnerDecision.mockResolvedValue({});
    renderView();
    await screen.findByText("Owner delegation");
    await userEvent.click(screen.getByText("Create delegation"));
    await waitFor(() =>
      expect(mocks.createAgenticOwnerDecision).toHaveBeenCalledTimes(1),
    );
    const input = mocks.createAgenticOwnerDecision.mock.calls[0][0];
    expect(input.matter).toBe("LICENSE_IDENTITY_EVIDENCE_DELEGATION");
    expect(input.options).toEqual([
      "APPROVE_DELEGATION",
      "DENY_DELEGATION",
    ]);
    expect(input.evidence).toMatchObject({
      domain: "licenses-identity-evidence",
      economicCriterion: "no-mandatory-fee-commercial-use",
    });
  });

  it("shows NEEDS_EVIDENCE and DENY without a revoke of an active allow", async () => {
    mocks.listLicenseDecisions.mockResolvedValue({
      decisions: [
        {
          ...({
            id: "decision-1",
            delegationId: "delegation-1",
            projectId: "project-1",
            leadRequestId: "lead-1",
            leadReservationId: "reservation-1",
            leadActorSha256: "a".repeat(64),
            leadRequestSha256: "c".repeat(64),
            leadOutputSha256: "d".repeat(64),
            leadPrincipal: "lead",
            delegationMatter: "LICENSE_IDENTITY_EVIDENCE_DELEGATION",
            delegationOption: "APPROVE_DELEGATION",
            delegationRevision: 1,
            delegationPin: "github:NixOS/nixpkgs/ac62194c",
            delegationUseContext: "commercial-development",
            delegationCriterion: "no-mandatory-fee-commercial-use",
            delegationDeadline: "2026-12-01T00:00:00Z",
            identity: { name: "ed", version: "1.21.1" },
            licenseExpression: "GPL-2.0-or-later",
            selectedBranch: "GPL-2.0-or-later",
            softwareObligations: ["notice", "source"],
            documentation: [],
            documentationObligations: [],
            softwareVerdict: "ALLOWED",
            evidenceSha256: "b".repeat(64),
            identityFactsReceiptDigest: "f".repeat(64),
            contentSha256: "1".repeat(64),
            provenance: {},
            useContext: "commercial-development",
            commercialUse: false,
            paymentRequired: false,
            catalogSha256: "2".repeat(64),
            profileSha256: "3".repeat(64),
            policySha256: "4".repeat(64),
            specArtifact: {},
            inventoryArtifact: {},
            candidateArtifact: {},
            revision: 1,
            status: "ACTIVE",
            createdAt: "2026-11-20T00:00:00Z",
            revokedAt: null,
            revocationReason: null,
            revokedBy: null,
          } satisfies Record<string, unknown>),
          decision: "NEEDS_EVIDENCE",
        },
        {
          ...({
            id: "decision-2",
            delegationId: "delegation-1",
            projectId: "project-1",
            leadRequestId: "lead-1",
            leadReservationId: "reservation-1",
            leadActorSha256: "a".repeat(64),
            leadRequestSha256: "c".repeat(64),
            leadOutputSha256: "d".repeat(64),
            leadPrincipal: "lead",
            delegationMatter: "LICENSE_IDENTITY_EVIDENCE_DELEGATION",
            delegationOption: "APPROVE_DELEGATION",
            delegationRevision: 1,
            delegationPin: "github:NixOS/nixpkgs/ac62194c",
            delegationUseContext: "commercial-development",
            delegationCriterion: "no-mandatory-fee-commercial-use",
            delegationDeadline: "2026-12-01T00:00:00Z",
            identity: { name: "ed", version: "1.21.1" },
            licenseExpression: "GPL-2.0-or-later",
            selectedBranch: "GPL-2.0-or-later",
            softwareObligations: ["notice", "source"],
            documentation: [],
            documentationObligations: [],
            softwareVerdict: "DENIED",
            evidenceSha256: "b".repeat(64),
            identityFactsReceiptDigest: "f".repeat(64),
            contentSha256: "1".repeat(64),
            provenance: {},
            useContext: "commercial-development",
            commercialUse: false,
            paymentRequired: true,
            catalogSha256: "2".repeat(64),
            profileSha256: "3".repeat(64),
            policySha256: "4".repeat(64),
            specArtifact: {},
            inventoryArtifact: {},
            candidateArtifact: {},
            revision: 1,
            status: "ACTIVE",
            createdAt: "2026-11-20T00:00:00Z",
            revokedAt: null,
            revocationReason: null,
            revokedBy: null,
          } satisfies Record<string, unknown>),
          decision: "DENY",
        },
      ],
    });
    renderView();
    expect(await screen.findByText("NEEDS_EVIDENCE")).toBeTruthy();
    expect(screen.getByText("DENY")).toBeTruthy();
  });

  it("shows a Core error with retry when the project load fails", async () => {
    mocks.listProjects.mockRejectedValue(new Error("core unavailable"));
    renderView();
    expect(await screen.findByText("Licence decisions could not be loaded.")).toBeTruthy();
    await userEvent.click(screen.getByText("Retry"));
    expect(mocks.listProjects).toHaveBeenCalledTimes(2);
  });

  it("shows a Core error when delegation creation fails", async () => {
    mocks.createAgenticOwnerDecision.mockRejectedValue(new Error("forbidden"));
    renderView();
    await screen.findByText("Owner delegation");
    await userEvent.click(screen.getByText("Create delegation"));
    expect(await screen.findByText("The delegation action failed.")).toBeTruthy();
  });

  it("shows a revoked exact decision without offering a second revoke", async () => {
    mocks.listLicenseDecisions.mockResolvedValue({
      decisions: [
        {
          id: "decision-1",
          delegationId: "delegation-1",
          projectId: "project-1",
          leadRequestId: "lead-1",
          leadReservationId: "reservation-1",
          leadActorSha256: "a".repeat(64),
          identity: { name: "ed", version: "1.21.1" },
          licenseExpression: "GPL-2.0-or-later",
          selectedBranch: "GPL-2.0-or-later",
          leadRequestSha256: "c".repeat(64),
          leadOutputSha256: "d".repeat(64),
          leadPrincipal: "lead",
          delegationMatter: "LICENSE_IDENTITY_EVIDENCE_DELEGATION",
          delegationOption: "APPROVE_DELEGATION",
          delegationRevision: 1,
          delegationPin: "github:NixOS/nixpkgs/ac62194c",
          delegationUseContext: "commercial-development",
          delegationCriterion: "no-mandatory-fee-commercial-use",
          delegationDeadline: "2026-12-01T00:00:00Z",
          softwareObligations: ["notice", "source"],
          documentation: [
            {
              path: "doc/fdl.texi",
              license: "GFDL-1.3-or-later",
              licenseTextSha256: "e".repeat(64),
              obligations: ["notice", "source"],
              verdict: "ALLOWED",
            },
          ],
          documentationObligations: ["notice", "source"],
          softwareVerdict: "ALLOWED",
          evidenceSha256: "b".repeat(64),
          identityFactsReceiptDigest: "f".repeat(64),
          contentSha256: "1".repeat(64),
          provenance: {},
          useContext: "commercial-development",
          commercialUse: true,
          paymentRequired: false,
          catalogSha256: "2".repeat(64),
          profileSha256: "3".repeat(64),
          policySha256: "4".repeat(64),
          specArtifact: {},
          inventoryArtifact: {},
          candidateArtifact: {},
          decision: "ACCEPT",
          revision: 1,
          status: "REVOKED",
          createdAt: "2026-11-20T00:00:00Z",
          revokedAt: "2026-11-20T01:00:00Z",
          revocationReason: "superseded",
          revokedBy: "operator",
        },
      ],
    });
    renderView();
    expect(await screen.findByText("REVOKED")).toBeTruthy();
    expect(screen.queryByText("Revoke")).toBeNull();
  });
});
