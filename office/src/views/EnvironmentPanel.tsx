import { useState } from "react";
import { Badge, type BadgeTone } from "../components/ui/Badge";
import { Button } from "../components/ui/Button";
import { Textarea } from "../components/ui/Field";
import { EmptyState, Panel } from "../components/ui/Surface";
import {
  CoreApiError,
  coreApi,
  type EnvironmentPlanStatus,
  type EnvironmentStateView,
} from "../coreApi";
import { shortId } from "../format";
import { useI18n } from "../i18n";

function statusTone(status: string | null | undefined): BadgeTone {
  switch (status) {
    case "FROZEN":
      return "success";
    case "LEAD_APPROVED":
    case "CORE_VALIDATED":
      return "info";
    case "BLOCKED":
    case "REJECTED":
      return "danger";
    case "DRAFT":
      return "attention";
    default:
      return "neutral";
  }
}

function builderTone(status: string | null | undefined): BadgeTone {
  switch (status) {
    case "SUCCEEDED":
    case "READY":
      return "success";
    case "BUILDING":
      return "attention";
    case "FAILED":
      return "danger";
    default:
      return "neutral";
  }
}

function reviewDecisionTone(decision: string): BadgeTone {
  if (decision === "APPROVED") return "success";
  if (decision === "REJECTED") return "danger";
  return "attention";
}

function errorMessage(cause: unknown, fallback: string): string {
  return cause instanceof CoreApiError
    ? cause.message
    : cause instanceof Error
      ? cause.message
      : fallback;
}

export function EnvironmentPanel({
  environment,
  requestId,
  onReviewed,
}: {
  environment: EnvironmentStateView | null | undefined;
  requestId?: string;
  onReviewed?: (next: EnvironmentStateView) => void;
}) {
  const { t, formatDate } = useI18n();
  const [rationale, setRationale] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const plan = environment?.plan ?? null;
  const leadApproval = environment?.leadApproval ?? null;
  const canReview =
    Boolean(requestId) &&
    Boolean(plan) &&
    (environment?.status === "CORE_VALIDATED" ||
      environment?.status === "DRAFT") &&
    !leadApproval;

  async function submit(decision: "APPROVE" | "CHANGES_REQUESTED" | "REJECT") {
    if (!requestId || !plan) return;
    if (rationale.trim().length < 8) {
      setError(t("environment.review.rationaleRequired"));
      return;
    }
    setBusy(decision);
    setError(null);
    setNotice(null);
    try {
      const next = await coreApi.reviewAgenticEnvironment(requestId, {
        planSha256: plan.planSha256,
        planRevision: plan.revision,
        decision,
        rationale: rationale.trim(),
      });
      setNotice(t("environment.review.success"));
      setRationale("");
      onReviewed?.(next);
    } catch (cause) {
      setError(errorMessage(cause, t("environment.review.error")));
    } finally {
      setBusy(null);
    }
  }

  if (!environment) {
    return (
      <Panel className="request-panel environment-panel" as="section">
        <div className="section-heading">
          <h2>{t("environment.title")}</h2>
          <Badge tone="neutral">{t("environment.none")}</Badge>
        </div>
        <EmptyState
          title={t("environment.emptyTitle")}
          description={t("environment.emptyDescription")}
        />
      </Panel>
    );
  }

  return (
    <Panel className="request-panel environment-panel" as="section">
      <div className="section-heading">
        <h2>{t("environment.title")}</h2>
        <Badge tone={statusTone(environment.status)}>
          {environment.status}
        </Badge>
        {environment.required && (
          <Badge tone="info">{t("environment.required")}</Badge>
        )}
      </div>

      {environment.blockers.length > 0 && (
        <div className="ui-callout ui-callout--danger" role="alert">
          <p>
            <strong>{t("environment.blockers.title")}</strong>
          </p>
          <ul>
            {environment.blockers.map((blocker) => (
              <li key={`${blocker.code}-${blocker.message}`}>
                <code>{blocker.code}</code> {blocker.message}
              </li>
            ))}
          </ul>
        </div>
      )}

      {!plan ? (
        <EmptyState
          title={t("environment.emptyTitle")}
          description={t("environment.emptyDescription")}
        />
      ) : (
        <>
          <dl className="request-facts">
            <div>
              <dt>{t("environment.planId")}</dt>
              <dd>
                <code title={plan.planId}>{healthShort(plan.planId)}</code>
              </dd>
            </div>
            <div>
              <dt>{t("environment.planHash")}</dt>
              <dd>
                <code title={plan.planSha256}>{plan.planSha256}</code>
              </dd>
            </div>
            <div>
              <dt>{t("environment.revision")}</dt>
              <dd>{plan.revision}</dd>
            </div>
            <div>
              <dt>{t("environment.risk")}</dt>
              <dd>{plan.riskClass}</dd>
            </div>
            <div>
              <dt>{t("environment.source")}</dt>
              <dd>{plan.source}</dd>
            </div>
            <div>
              <dt>{t("environment.baseCommit")}</dt>
              <dd>
                <code title={plan.baseCommit}>
                  {shortId(plan.baseCommit, 12)}
                </code>
              </dd>
            </div>
            <div>
              <dt>{t("environment.conventionsHash")}</dt>
              <dd>
                <code title={plan.conventionsSha256}>
                  {shortId(plan.conventionsSha256, 16)}
                </code>
              </dd>
            </div>
          </dl>

          <h3 className="environment-heading">
            {t("environment.detection.title")}
          </h3>
          <p className="muted">{t("environment.detection.description")}</p>
          {plan.conventions.files.length === 0 ? (
            <p className="muted">{t("environment.detection.empty")}</p>
          ) : (
            <div className="environment-table-scroll">
              <table className="environment-table">
                <caption>{t("environment.detection.caption")}</caption>
                <thead>
                  <tr>
                    <th scope="col">{t("environment.detection.path")}</th>
                    <th scope="col">{t("environment.detection.kind")}</th>
                    <th scope="col">{t("environment.detection.precedence")}</th>
                    <th scope="col">{t("environment.detection.pins")}</th>
                  </tr>
                </thead>
                <tbody>
                  {plan.conventions.files.map((file) => (
                    <tr key={file.path}>
                      <td>
                        <code>{file.path}</code>
                      </td>
                      <td>{file.kind}</td>
                      <td>{file.precedence}</td>
                      <td>
                        {file.pins.length === 0
                          ? "—"
                          : file.pins
                              .map((pin) => `${pin.name}=${pin.value}`)
                              .join(", ")}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
          {plan.conventions.conflicts.length > 0 && (
            <div className="ui-callout ui-callout--danger" role="alert">
              <p>
                <strong>{t("environment.conflicts.title")}</strong>
              </p>
              <ul>
                {plan.conventions.conflicts.map((conflict) => (
                  <li key={conflict.name}>
                    <code>{conflict.name}</code> {conflict.detail}{" "}
                    {conflict.values.join(" / ")} ({conflict.sources.join(", ")}
                    )
                  </li>
                ))}
              </ul>
            </div>
          )}

          <h3 className="environment-heading">
            {t("environment.selected.title")}
          </h3>
          <dl className="request-facts request-facts--wide">
            <div>
              <dt>{t("environment.selected.toolchains")}</dt>
              <dd>
                {plan.toolchains.length === 0
                  ? t("environment.selected.none")
                  : plan.toolchains
                      .map(
                        (toolchain) => `${toolchain.id} ${toolchain.version}`,
                      )
                      .join("; ")}
              </dd>
            </div>
            <div>
              <dt>{t("environment.selected.packages")}</dt>
              <dd>
                {plan.packages.length === 0
                  ? t("environment.selected.none")
                  : plan.packages
                      .map((item) => `${item.name} ${item.version}`)
                      .join("; ")}
              </dd>
            </div>
            <div>
              <dt>{t("environment.selected.services")}</dt>
              <dd>
                {plan.services.length === 0
                  ? t("environment.selected.none")
                  : plan.services
                      .map((item) => `${item.name} ${item.version}`)
                      .join("; ")}
              </dd>
            </div>
            <div>
              <dt>{t("environment.selected.browsers")}</dt>
              <dd>
                {plan.browsers.length === 0
                  ? t("environment.selected.none")
                  : plan.browsers
                      .map((item) => `${item.name} ${item.version}`)
                      .join("; ")}
              </dd>
            </div>
            <div>
              <dt>{t("environment.selected.nixpkgs")}</dt>
              <dd>
                <code>{plan.nix.nixpkgs || "—"}</code>
              </dd>
            </div>
            <div>
              <dt>{t("environment.selected.allowUnfree")}</dt>
              <dd>
                {plan.nix.allowUnfree
                  ? t("environment.yes")
                  : t("environment.no")}
              </dd>
            </div>
            <div>
              <dt>{t("environment.selected.resources")}</dt>
              <dd>
                {t("environment.selected.resourceValue", {
                  cpu: plan.resources.cpu,
                  memory: plan.resources.memoryMib,
                  timeout: plan.resources.timeoutSecs,
                })}
              </dd>
            </div>
            <div>
              <dt>{t("environment.selected.network")}</dt>
              <dd>
                {plan.network.build} → {plan.network.fetch}
              </dd>
            </div>
          </dl>
          {plan.commandsRationale && (
            <p className="muted">{plan.commandsRationale}</p>
          )}

          <h3 className="environment-heading">
            {t("environment.license.title")}
          </h3>
          <p className="muted">{t("environment.license.description")}</p>
          {plan.licenseInventory.entries.length === 0 ? (
            <p className="muted">{t("environment.license.empty")}</p>
          ) : (
            <div className="environment-table-scroll">
              <table className="environment-table">
                <caption>{t("environment.license.caption")}</caption>
                <thead>
                  <tr>
                    <th scope="col">{t("environment.license.name")}</th>
                    <th scope="col">{t("environment.license.version")}</th>
                    <th scope="col">{t("environment.license.license")}</th>
                    <th scope="col">{t("environment.license.unfree")}</th>
                    <th scope="col">{t("environment.license.override")}</th>
                  </tr>
                </thead>
                <tbody>
                  {plan.licenseInventory.entries.map((entry) => (
                    <tr key={`${entry.name}-${entry.version}`}>
                      <td>{entry.name}</td>
                      <td>{entry.version}</td>
                      <td>{entry.license}</td>
                      <td>
                        {entry.unfree
                          ? t("environment.yes")
                          : t("environment.no")}
                      </td>
                      <td>
                        {entry.ownerOverrideId ? (
                          <code>{shortId(entry.ownerOverrideId, 12)}</code>
                        ) : (
                          "—"
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
          {plan.licenseInventory.blockers.length > 0 && (
            <div className="ui-callout ui-callout--danger" role="alert">
              <p>
                <strong>{t("environment.license.blockersTitle")}</strong>
              </p>
              <ul>
                {plan.licenseInventory.blockers.map((blocker) => (
                  <li key={`${blocker.code}-${blocker.message}`}>
                    <code>{blocker.code}</code> {blocker.message}
                  </li>
                ))}
              </ul>
            </div>
          )}

          <h3 className="environment-heading">
            {t("environment.builder.title")}
          </h3>
          <dl className="request-facts">
            <div>
              <dt>{t("environment.builder.status")}</dt>
              <dd>
                <Badge
                  tone={builderTone(environment.builder.descriptor?.status)}
                >
                  {environment.builder.descriptor?.status ?? "UNAVAILABLE"}
                </Badge>
              </dd>
            </div>
            <div>
              <dt>{t("environment.builder.image")}</dt>
              <dd>
                <code>{environment.builder.descriptor?.image || "—"}</code>
              </dd>
            </div>
            <div>
              <dt>{t("environment.builder.digest")}</dt>
              <dd>
                <code>{environment.builder.descriptor?.digest || "—"}</code>
              </dd>
            </div>
            <div>
              <dt>{t("environment.builder.receipt")}</dt>
              <dd>
                {environment.builder.receipt
                  ? `${environment.builder.receipt.status} (${shortId(environment.builder.receipt.outputSha256, 12)})`
                  : t("environment.builder.noReceipt")}
              </dd>
            </div>
          </dl>

          <h3 className="environment-heading">
            {t("environment.review.title")}
          </h3>
          {leadApproval ? (
            <div className="environment-review-record">
              <Badge tone={reviewDecisionTone(leadApproval.decision)}>
                {leadApproval.decision}
              </Badge>
              <dl className="request-facts">
                <div>
                  <dt>{t("environment.review.principal")}</dt>
                  <dd>
                    <code>{leadApproval.principalId}</code>
                  </dd>
                </div>
                <div>
                  <dt>{t("environment.review.recordedBy")}</dt>
                  <dd>{leadApproval.recordedBy}</dd>
                </div>
                <div>
                  <dt>{t("environment.review.decidedAt")}</dt>
                  <dd>{formatDate(leadApproval.decidedAt)}</dd>
                </div>
              </dl>
              <p className="muted">{leadApproval.rationale}</p>
              <p className="muted">{t("environment.review.notAuthority")}</p>
            </div>
          ) : (
            <p className="muted">{t("environment.review.pending")}</p>
          )}

          {canReview && (
            <div className="environment-review-form">
              <Textarea
                label={t("environment.review.rationale")}
                value={rationale}
                onChange={(event) => setRationale(event.target.value)}
                help={t("environment.review.rationaleHelp")}
                disabled={busy !== null}
              />
              <div className="environment-review-actions">
                <Button
                  variant="primary"
                  busy={busy === "APPROVE"}
                  disabled={busy !== null || rationale.trim().length < 8}
                  onClick={() => void submit("APPROVE")}
                >
                  {t("environment.review.approve")}
                </Button>
                <Button
                  variant="secondary"
                  busy={busy === "CHANGES_REQUESTED"}
                  disabled={busy !== null || rationale.trim().length < 8}
                  onClick={() => void submit("CHANGES_REQUESTED")}
                >
                  {t("environment.review.requestChanges")}
                </Button>
                <Button
                  variant="danger"
                  busy={busy === "REJECT"}
                  disabled={busy !== null || rationale.trim().length < 8}
                  onClick={() => void submit("REJECT")}
                >
                  {t("environment.review.reject")}
                </Button>
              </div>
              <p className="muted">{t("environment.review.actionHelp")}</p>
            </div>
          )}

          {error && (
            <p className="ui-callout ui-callout--danger" role="alert">
              {error}
            </p>
          )}
          {notice && (
            <p className="notice" role="status" aria-live="polite">
              {notice}
            </p>
          )}

          {environment.spec && (
            <>
              <h3 className="environment-heading">
                {t("environment.spec.title")}
              </h3>
              <dl className="request-facts">
                <div>
                  <dt>{t("environment.spec.hash")}</dt>
                  <dd>
                    <code title={environment.spec.specSha256}>
                      {environment.spec.specSha256}
                    </code>
                  </dd>
                </div>
                <div>
                  <dt>{t("environment.spec.frozenAt")}</dt>
                  <dd>{formatDate(environment.spec.frozenAt)}</dd>
                </div>
                <div>
                  <dt>{t("environment.spec.flakeLock")}</dt>
                  <dd>
                    <code>{environment.spec.flakeLockSha256 || "—"}</code>
                  </dd>
                </div>
                <div>
                  <dt>{t("environment.spec.devenvLock")}</dt>
                  <dd>
                    <code>{environment.spec.devenvLockSha256 || "—"}</code>
                  </dd>
                </div>
                <div>
                  <dt>{t("environment.spec.licenses")}</dt>
                  <dd>
                    <code>{environment.spec.licenseInventorySha256}</code>
                  </dd>
                </div>
                <div>
                  <dt>{t("environment.spec.sbom")}</dt>
                  <dd>
                    <code>{environment.spec.sbomSha256}</code>
                  </dd>
                </div>
              </dl>
              <p className="muted">{t("environment.spec.immutable")}</p>
            </>
          )}
        </>
      )}
    </Panel>
  );
}

function healthShort(value: string): string {
  return value.length <= 16 ? value : `${value.slice(0, 8)}…${value.slice(-4)}`;
}

export function environmentStatusLabel(
  status: EnvironmentPlanStatus | string | null | undefined,
  t: ReturnType<typeof useI18n>["t"],
): string {
  switch (status) {
    case "FROZEN":
      return t("environment.status.frozen");
    case "LEAD_APPROVED":
      return t("environment.status.leadApproved");
    case "CORE_VALIDATED":
      return t("environment.status.coreValidated");
    case "BLOCKED":
      return t("environment.status.blocked");
    case "REJECTED":
      return t("environment.status.rejected");
    case "MISSING":
      return t("environment.status.missing");
    default:
      return t("environment.status.draft");
  }
}
