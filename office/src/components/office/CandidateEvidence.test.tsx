import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { Gate, QualityReceipt, ReviewFinding } from "../../coreApi";
import { I18nProvider } from "../../i18n";
import {
  CandidateFinalSourceBadge,
  QualityGatePanel,
  ReceiptModelProvenance,
  ReviewFindings,
} from "./CandidateEvidence";

const critical: ReviewFinding = {
  id: "critical-1",
  severity: "critical",
  category: "security",
  message: "credential boundary is broken",
  points: 8,
  rationale: "The exact diff exposes a credential boundary.",
  evidenceRefs: ["src/auth.rs:42"],
};

function receipt(overrides: Partial<QualityReceipt> = {}): QualityReceipt {
  return {
    receipt_id: "reviewer-1",
    candidate_id: "candidate-1",
    role: "reviewer",
    principal_id: "reviewer",
    action_spec_sha256: "a".repeat(64),
    commit_sha: "b".repeat(40),
    tree_sha: "c".repeat(40),
    result: "CHANGES_REQUESTED",
    evidence: "EXECUTED",
    findings: [critical],
    mechanism: "CODEX_APP_SERVER",
    model_provider: "codex",
    model_name: "gpt-6-luna",
    model_reservation_id: "",
    model_request_sha256: "d".repeat(64),
    input_tokens: 9064,
    output_tokens: 97,
    total_tokens: 9161,
    cost_micros: 0,
    usage_known: true,
    received_at: "2026-11-06T21:38:37Z",
    ...overrides,
  };
}

const gate: Gate = {
  candidate_id: "candidate-1",
  outcome: "HUMAN_REQUIRED",
  reasons: ["critical review finding requires human review"],
  decided_at: "2026-11-06T21:38:37Z",
  repair_authorization: {
    repair_id: "repair-1",
    campaign_id: "campaign-1",
    candidate_id: "candidate-1",
    work_item_id: "unit-1",
    repair_trigger: "QUALITY_GATE",
    risk_class: "R0",
    total_points: 8,
    critical_count: 1,
    blocking_count: 0,
    warning_count: 0,
    info_count: 0,
    cycle_index: 0,
    decision: "HUMAN_REQUIRED",
    reason_code: "critical_zero",
    reason: "a critical finding always requires human review",
    policy_sha256: "e".repeat(64),
    reviewer_receipt_id: "reviewer-1",
    created_at: "2026-11-06T21:38:37Z",
  },
};

function gateWithOutcome(outcome: Gate["outcome"]): Gate {
  return { ...gate, outcome };
}

describe("CandidateEvidence", () => {
  it("renders canonical finding points, rationale and native provenance", () => {
    render(
      <I18nProvider>
        <ReviewFindings receipts={[receipt()]} />
        <ReceiptModelProvenance receipts={[receipt()]} />
      </I18nProvider>,
    );
    expect(screen.getByText("8 points")).toBeDefined();
    expect(
      screen.getByText("The exact diff exposes a credential boundary."),
    ).toBeDefined();
    expect(screen.getByText(/src\/auth\.rs:42/)).toBeDefined();
    expect(screen.getByText(/codex\/gpt-6-luna/)).toBeDefined();
    expect(screen.getByText(/9,064 in/)).toBeDefined();
  });

  it("renders a deterministic repair authorization from Core", () => {
    render(
      <I18nProvider>
        <QualityGatePanel gate={gate} candidateId="candidate-1" />
      </I18nProvider>,
    );
    expect(screen.getByText("Repair authorization")).toBeDefined();
    expect(screen.getAllByText("Human required").length).toBeGreaterThan(0);
    expect(
      screen.getByText(/a critical finding always requires human review/),
    ).toBeDefined();
  });

  it("hides a historical repair authorization once the gate is accepted", () => {
    render(
      <I18nProvider>
        <QualityGatePanel
          gate={gateWithOutcome("ACCEPTED")}
          candidateId="candidate-1"
        />
      </I18nProvider>,
    );
    expect(screen.queryByText("Repair authorization")).toBeNull();
    expect(screen.getByText("Accepted")).toBeDefined();
    expect(
      screen.getByText("critical review finding requires human review"),
    ).toBeDefined();
  });

  it("still renders the repair authorization while repair is required", () => {
    render(
      <I18nProvider>
        <QualityGatePanel
          gate={gateWithOutcome("REPAIR_REQUIRED")}
          candidateId="candidate-1"
        />
      </I18nProvider>,
    );
    expect(screen.getByText("Repair authorization")).toBeDefined();
  });

  it("labels a stagnation salvage candidate truthfully", () => {
    render(
      <I18nProvider>
        <CandidateFinalSourceBadge source="STAGNATION_SALVAGE" />
        <QualityGatePanel
          gate={{
            ...gateWithOutcome("HUMAN_REQUIRED"),
            final_source: "STAGNATION_SALVAGE",
            reasons: [
              "SALVAGE_HUMAN_REVIEW_REQUIRED: R1+ stagnation salvage requires an authenticated owner/human review before acceptance",
            ],
          }}
          candidateId="candidate-1"
        />
      </I18nProvider>,
    );
    expect(screen.getAllByText("Stagnation salvage").length).toBe(2);
    expect(
      screen.getAllByText(/the model loop had stopped/).length,
    ).toBeGreaterThan(0);
    expect(screen.getByText(/SALVAGE_HUMAN_REVIEW_REQUIRED/)).toBeDefined();
  });
});
