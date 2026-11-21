import { useCallback, useEffect, useState } from "react";
import { Badge, type BadgeTone } from "../components/ui/Badge";
import { Button } from "../components/ui/Button";
import { Input, Select } from "../components/ui/Field";
import {
  EmptyState,
  ErrorState,
  LoadingState,
  PageHeader,
  Panel,
} from "../components/ui/Surface";
import {
  type AgenticOwnerDecision,
  CoreApiError,
  coreApi,
  type IntakeRequestSummary,
  type LicenseDecisionRecord,
  type Project,
} from "../coreApi";
import { useI18n } from "../i18n";

function errorText(cause: unknown, fallback: string): string {
  return cause instanceof CoreApiError ? cause.message : fallback;
}

function decisionTone(decision: string): BadgeTone {
  if (decision === "ACCEPT") return "success";
  if (decision === "DENY") return "danger";
  return "attention";
}

function shortHash(value: unknown): string {
  return typeof value === "string" && value.length > 0
    ? value.slice(0, 12)
    : "—";
}

function decisionStage(provenance: Record<string, unknown>): string {
  const stage = provenance.stage;
  if (!stage || typeof stage !== "object") return "—";
  const record = stage as { stage?: unknown; planSha256?: unknown };
  return [record.stage, shortHash(record.planSha256)]
    .filter((part) => typeof part === "string" && part !== "—")
    .join(" · ") || "—";
}

function sourceProof(provenance: Record<string, unknown>): string {
  return shortHash(provenance.sourceEvidenceSha256);
}

function defaultExpiry(): string {
  return new Date(Date.now() + 7 * 24 * 60 * 60 * 1000)
    .toISOString()
    .slice(0, 10);
}

export function LicenseDecisions() {
  const { t, formatDate } = useI18n();
  const [projects, setProjects] = useState<Project[]>([]);
  const [projectId, setProjectId] = useState("");
  const [delegations, setDelegations] = useState<AgenticOwnerDecision[]>([]);
  const [decisions, setDecisions] = useState<LicenseDecisionRecord[]>([]);
  const [requests, setRequests] = useState<IntakeRequestSummary[]>([]);
  const [requestId, setRequestId] = useState("");
  const [review, setReview] = useState<{
    reservationId: string;
    decision: string;
    name: string;
    version: string;
    licenseExpression: string;
    selectedBranch: string;
    paymentRequired: boolean;
  } | null>(null);
  const [reviewBusy, setReviewBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [pin, setPin] = useState(
    "github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d",
  );
  const [useContext, setUseContext] = useState("commercial-development");
  const [expiry, setExpiry] = useState(defaultExpiry);
  const [reasons, setReasons] = useState<Record<string, string>>({});

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const projectResult = await coreApi.listProjects();
      setProjects(projectResult);
      const selected = projectId || projectResult[0]?.id || "";
      if (!projectId && selected) setProjectId(selected);
      if (selected) {
        const [ownerDecisions, licenseDecisions, requestList] =
          await Promise.all([
            coreApi.listAgenticDecisions(selected),
            coreApi.listLicenseDecisions(selected),
            coreApi.listIntakeRequests({ projectId: selected, limit: 20 }),
          ]);
        setDelegations(
          ownerDecisions.decisions.filter(
            (decision) =>
              decision.matter === "LICENSE_IDENTITY_EVIDENCE_DELEGATION",
          ),
        );
        setDecisions(licenseDecisions.decisions);
        setRequests(requestList);
        if (!requestId) setRequestId(requestList[0]?.id ?? "");
      } else {
        setDelegations([]);
        setDecisions([]);
        setRequests([]);
        setRequestId("");
      }
      setError(null);
    } catch (cause) {
      setError(errorText(cause, t("licenses.error")));
    } finally {
      setLoading(false);
    }
  }, [projectId, t]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    let cancelled = false;
    if (!requestId) {
      setReview(null);
      return;
    }
    coreApi
      .getLicenseEvidenceReview(requestId)
      .then((status) => {
        if (cancelled) return;
        setReview(
          status.available &&
            status.reservationId &&
            status.review
            ? {
                reservationId: status.reservationId,
                decision: status.review.decision,
                name: status.review.name,
                version: status.review.version,
                licenseExpression: status.review.licenseExpression,
                selectedBranch: status.review.selectedBranch,
                paymentRequired: status.review.paymentRequired,
              }
            : null,
        );
      })
      .catch(() => {
        if (!cancelled) setReview(null);
      });
    return () => {
      cancelled = true;
    };
  }, [requestId]);

  async function createDelegation() {
    if (!projectId || pin.trim().length === 0 || useContext.trim().length === 0) {
      return;
    }
    setBusy("create");
    setNotice(null);
    try {
      await coreApi.createAgenticOwnerDecision({
        project_id: projectId,
        matter: "LICENSE_IDENTITY_EVIDENCE_DELEGATION",
        risk: "R0",
        options: ["APPROVE_DELEGATION", "DENY_DELEGATION"],
        evidence: {
          domain: "licenses-identity-evidence",
          pin: pin.trim(),
          useContext: useContext.trim(),
          economicCriterion: "no-mandatory-fee-commercial-use",
          revision: 1,
          scope: "exact-identity",
        },
        recommendation: "APPROVE_DELEGATION",
        deadline: new Date(`${expiry}T23:59:59Z`).toISOString(),
      });
      setNotice(t("licenses.delegation.created"));
      await load();
    } catch (cause) {
      setError(errorText(cause, t("licenses.delegation.error")));
    } finally {
      setBusy(null);
    }
  }

  async function resolve(delegation: AgenticOwnerDecision) {
    setBusy(delegation.id);
    setNotice(null);
    try {
      await coreApi.resolveAgenticOwnerDecision(
        delegation.id,
        "APPROVE_DELEGATION",
        "Owner delegated the exact licence, identity and evidence decision.",
      );
      setNotice(t("licenses.delegation.approved"));
      await load();
    } catch (cause) {
      setError(errorText(cause, t("licenses.delegation.error")));
    } finally {
      setBusy(null);
    }
  }

  async function revokeDelegation(delegation: AgenticOwnerDecision) {
    const reason = (reasons[delegation.id] ?? "").trim();
    if (reason.length < 4) return;
    setBusy(delegation.id);
    setNotice(null);
    try {
      await coreApi.revokeLicenseDelegation(delegation.id, reason);
      setNotice(t("licenses.delegation.revoked"));
      await load();
    } catch (cause) {
      setError(errorText(cause, t("licenses.delegation.error")));
    } finally {
      setBusy(null);
    }
  }

  async function revokeDecision(decision: LicenseDecisionRecord) {
    const reason = (reasons[decision.id] ?? "").trim();
    if (reason.length < 4) return;
    setBusy(decision.id);
    setNotice(null);
    try {
      await coreApi.revokeLicenseDecision(decision.id, reason);
      setNotice(t("licenses.decision.revoked"));
      await load();
    } catch (cause) {
      setError(errorText(cause, t("licenses.error")));
    } finally {
      setBusy(null);
    }
  }

  const approvedDelegation = delegations.find(
    (delegation) =>
      delegation.status === "APPROVED" &&
      delegation.selectedOption === "APPROVE_DELEGATION",
  );

  async function runReview() {
    if (!requestId || !approvedDelegation) return;
    setReviewBusy(true);
    setNotice(null);
    try {
      const result = await coreApi.requestLicenseEvidenceReview(requestId, {
        delegation_id: approvedDelegation.id,
        idempotency_key: `license-review:${requestId}:${approvedDelegation.id}`,
      });
      setReview({
        reservationId: result.reservationId,
        decision: result.review.decision,
        name: result.review.name,
        version: result.review.version,
        licenseExpression: result.review.licenseExpression,
        selectedBranch: result.review.selectedBranch,
        paymentRequired: result.review.paymentRequired,
      });
      setNotice(t("licenses.review.available"));
    } catch (cause) {
      setError(errorText(cause, t("licenses.review.error")));
    } finally {
      setReviewBusy(false);
    }
  }

  async function recordDecision() {
    if (!requestId || !approvedDelegation || !review) return;
    setReviewBusy(true);
    setNotice(null);
    try {
      await coreApi.recordLicenseDecision(requestId, {
        delegation_id: approvedDelegation.id,
        idempotency_key: `license-decision:${review.reservationId}`,
      });
      setNotice(t("licenses.review.recorded"));
      await load();
    } catch (cause) {
      setError(errorText(cause, t("licenses.review.error")));
    } finally {
      setReviewBusy(false);
    }
  }

  if (loading) return <LoadingState label={t("licenses.loading")} />;
  if (error && projects.length === 0)
    return (
      <ErrorState
        message={error}
        action={
          <Button onClick={() => void load()}>{t("licenses.retry")}</Button>
        }
      />
    );

  return (
    <div className="stack">
      <PageHeader
        title={t("licenses.title")}
        description={t("licenses.description")}
      />
      {notice ? <p className="notice">{notice}</p> : null}
      {error ? <p className="form-error">{error}</p> : null}
      <Panel>
        <h2 className="ui-section-title">{t("licenses.delegation.title")}</h2>
        <div className="inline-form">
          <Select
            label={t("licenses.project")}
            value={projectId}
            onChange={(event) => setProjectId(event.target.value)}
          >
            {projects.map((project) => (
              <option key={project.id} value={project.id}>
                {project.name ?? project.repository}
              </option>
            ))}
          </Select>
          <Input
            label={t("licenses.pin")}
            value={pin}
            onChange={(event) => setPin(event.target.value)}
          />
          <Input
            label={t("licenses.useContext")}
            value={useContext}
            onChange={(event) => setUseContext(event.target.value)}
          />
          <Input
            label={t("licenses.expiry")}
            type="date"
            value={expiry}
            onChange={(event) => setExpiry(event.target.value)}
          />
          <Button
            onClick={() => void createDelegation()}
            disabled={busy !== null || !projectId}
          >
            {t("licenses.delegation.create")}
          </Button>
        </div>
        {delegations.length === 0 ? (
          <EmptyState
            title={t("licenses.delegation.emptyTitle")}
            description={t("licenses.delegation.emptyDescription")}
          />
        ) : (
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>{t("licenses.status")}</th>
                  <th>{t("licenses.pin")}</th>
                  <th>{t("licenses.useContext")}</th>
                  <th>{t("licenses.expiry")}</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {delegations.map((delegation) => {
                  const evidence = delegation.evidence as {
                    pin?: string;
                    useContext?: string;
                  };
                  return (
                    <tr key={delegation.id}>
                      <td>
                        <Badge tone={decisionTone(delegation.status === "APPROVED" ? "ACCEPT" : delegation.status === "DENIED" ? "DENY" : "NEEDS_EVIDENCE")}>
                          {delegation.status}
                        </Badge>
                      </td>
                      <td className="mono">{evidence?.pin ?? "—"}</td>
                      <td>{evidence?.useContext ?? "—"}</td>
                      <td>{formatDate(delegation.deadline)}</td>
                      <td>
                        {delegation.status === "PENDING" ? (
                          <Button
                            onClick={() => void resolve(delegation)}
                            disabled={busy !== null}
                          >
                            {t("licenses.delegation.approve")}
                          </Button>
                        ) : (
                          <div className="stack-xs">
                            <Input
                              label={t("licenses.reason")}
                              value={reasons[delegation.id] ?? ""}
                              onChange={(event) =>
                                setReasons((current) => ({
                                  ...current,
                                  [delegation.id]: event.target.value,
                                }))
                              }
                            />
                            <Button
                              variant="danger"
                              onClick={() => void revokeDelegation(delegation)}
                              disabled={
                                busy !== null ||
                                (reasons[delegation.id] ?? "").trim().length < 4
                              }
                            >
                              {t("licenses.revoke")}
                            </Button>
                          </div>
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </Panel>
      <Panel>
        <h2 className="ui-section-title">{t("licenses.review.title")}</h2>
        {requests.length === 0 ? (
          <EmptyState
            title={t("licenses.review.emptyTitle")}
            description={t("licenses.review.emptyDescription")}
          />
        ) : (
          <div className="stack-xs">
            <Select
              label={t("licenses.review.request")}
              value={requestId}
              onChange={(event) => setRequestId(event.target.value)}
            >
              {requests.map((request) => (
                <option key={request.id} value={request.id}>
                  {request.objective} · {request.state}
                </option>
              ))}
            </Select>
            {approvedDelegation ? null : (
              <p className="muted">{t("licenses.review.delegationRequired")}</p>
            )}
            {review ? (
              <div className="stack-xs">
                <p>
                  <Badge tone={decisionTone(review.decision)}>
                    {review.decision}
                  </Badge>{" "}
                  {review.name} {review.version} ·{" "}
                  <span className="mono">{review.licenseExpression}</span> ·{" "}
                  {review.selectedBranch}
                  {review.paymentRequired
                    ? ` · ${t("licenses.review.paid")}`
                    : ""}
                </p>
                <Button
                  onClick={() => void recordDecision()}
                  disabled={reviewBusy || !approvedDelegation}
                >
                  {t("licenses.review.record")}
                </Button>
              </div>
            ) : (
              <Button
                onClick={() => void runReview()}
                disabled={reviewBusy || !approvedDelegation}
              >
                {t("licenses.review.run")}
              </Button>
            )}
            {approvedDelegation ? null : (
              <p className="muted">{t("licenses.review.required")}</p>
            )}
          </div>
        )}
      </Panel>
      <Panel>
        <h2 className="ui-section-title">{t("licenses.decisions.title")}</h2>
        {decisions.length === 0 ? (
          <EmptyState
            title={t("licenses.decisions.emptyTitle")}
            description={t("licenses.decisions.emptyDescription")}
          />
        ) : (
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>{t("licenses.identity")}</th>
                  <th>{t("licenses.license")}</th>
                  <th>{t("licenses.branch")}</th>
                  <th>{t("licenses.obligations")}</th>
                  <th>{t("licenses.useContext")}</th>
                  <th>{t("licenses.stage")}</th>
                  <th>{t("licenses.sourceProof")}</th>
                  <th>{t("licenses.status")}</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {decisions.map((decision) => (
                  <tr key={decision.id}>
                    <td>
                      {String(decision.identity.name ?? "—")}{" "}
                      {String(decision.identity.version ?? "")}
                    </td>
                    <td className="mono">{decision.licenseExpression}</td>
                    <td className="mono">{decision.selectedBranch}</td>
                    <td>
                      {decision.softwareObligations.join(", ")}
                      {decision.documentation.length > 0
                        ? ` · ${decision.documentation
                            .map(
                              (item) =>
                                `${item.path}: ${item.license} (${item.obligations.join(", ")})`,
                            )
                            .join("; ")}`
                        : ""}
                    </td>
                    <td>{decision.useContext}</td>
                    <td className="mono">{decisionStage(decision.provenance)}</td>
                    <td className="mono">{sourceProof(decision.provenance)}</td>
                    <td>
                      <Badge tone={decisionTone(decision.status === "REVOKED" ? "DENY" : decision.decision)}>
                        {decision.status === "REVOKED"
                          ? "REVOKED"
                          : decision.decision}
                      </Badge>
                      {decision.status === "REVOKED" && decision.revocationReason ? (
                        <div className="muted">
                          {decision.revocationReason}
                        </div>
                      ) : null}
                    </td>
                    <td>
                      {decision.status === "REVOKED" ? null : (
                        <div className="stack-xs">
                          <Input
                            label={t("licenses.reason")}
                            value={reasons[decision.id] ?? ""}
                            onChange={(event) =>
                              setReasons((current) => ({
                                ...current,
                                [decision.id]: event.target.value,
                              }))
                            }
                          />
                          <Button
                            variant="danger"
                            onClick={() => void revokeDecision(decision)}
                            disabled={
                              busy !== null ||
                              (reasons[decision.id] ?? "").trim().length < 4
                            }
                          >
                            {t("licenses.revoke")}
                          </Button>
                        </div>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Panel>
    </div>
  );
}
