import { type ReactNode, useEffect, useState } from "react";
import { Badge } from "../components/ui/Badge";
import { Button } from "../components/ui/Button";
import { ErrorState, LoadingState, Panel } from "../components/ui/Surface";
import { CoreApiError, coreApi, type RequestFundingStatus } from "../coreApi";
import { useI18n } from "../i18n";

type FundingLoadState =
  | { kind: "loading" }
  | { kind: "unavailable" }
  | { kind: "error"; message: string }
  | { kind: "ready"; data: RequestFundingStatus };

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div>
      <dt>{label}</dt>
      <dd>{children}</dd>
    </div>
  );
}

function ValueList({ values }: { values: string[] }) {
  const { t } = useI18n();
  if (values.length === 0) {
    return <span className="muted">{t("common.none")}</span>;
  }
  return (
    <ul className="request-value-list">
      {values.map((value) => (
        <li key={value}>
          <code>{value}</code>
        </li>
      ))}
    </ul>
  );
}

function verdictTone(
  data: RequestFundingStatus,
): "accent" | "attention" | "danger" {
  if (data.requestScopedAdmissible) return "accent";
  if (
    !data.requestScoped ||
    data.verdict === "ADMISSION_EXPIRED" ||
    data.verdict === "ADMISSION_INACTIVE" ||
    data.verdict === "EXTRA_USAGE_NOT_DISABLED" ||
    data.verdict === "PROVIDER_DISABLED" ||
    data.verdict === "NON_CANONICAL_ROUTE"
  ) {
    return "danger";
  }
  return "attention";
}

export function FundingReadinessPanel({
  requestId,
  refreshKey,
}: {
  requestId: string;
  refreshKey?: string | null;
}) {
  const { t } = useI18n();
  const [state, setState] = useState<FundingLoadState>({ kind: "loading" });
  const [reload, setReload] = useState(0);
  const loadKey = `${requestId}:${refreshKey ?? ""}:${reload}`;

  useEffect(() => {
    void loadKey;
    let active = true;
    setState({ kind: "loading" });
    coreApi
      .getIntakeFundingReadiness(requestId)
      .then((data) => {
        if (active) setState({ kind: "ready", data });
      })
      .catch((cause) => {
        if (!active) return;
        if (cause instanceof CoreApiError && cause.status === 404) {
          setState({ kind: "unavailable" });
          return;
        }
        setState({
          kind: "error",
          message:
            cause instanceof Error ? cause.message : t("request.funding.error"),
        });
      });
    return () => {
      active = false;
    };
  }, [loadKey, requestId, t]);

  return (
    <Panel className="request-panel" data-testid="funding-readiness">
      <div className="section-heading">
        <h2>{t("request.funding.title")}</h2>
        {state.kind === "ready" && (
          <Badge tone={verdictTone(state.data)}>{state.data.verdict}</Badge>
        )}
      </div>
      <p className="muted">{t("request.funding.hint")}</p>

      {state.kind === "loading" && (
        <LoadingState label={t("request.funding.loading")} />
      )}

      {state.kind === "unavailable" && (
        <p className="ui-callout ui-callout--attention" role="status">
          {t("request.funding.unavailable")}
        </p>
      )}

      {state.kind === "error" && (
        <ErrorState
          title={t("common.errorTitle")}
          message={state.message}
          action={
            <Button onClick={() => setReload((value) => value + 1)}>
              {t("common.retry")}
            </Button>
          }
        />
      )}

      {state.kind === "ready" && <FundingStatus data={state.data} />}
    </Panel>
  );
}

function FundingStatus({ data }: { data: RequestFundingStatus }) {
  const { t, formatDate } = useI18n();
  return (
    <div className="request-funding">
      {!data.requestScoped && (
        <p className="ui-callout ui-callout--danger" role="alert">
          {t("request.funding.unscopedWarning")}
        </p>
      )}
      {data.requestScoped && data.requestScopedAdmissible && (
        <p className="ui-callout ui-callout--info" role="status">
          {t("request.funding.admitted")}
        </p>
      )}
      {data.requestScoped && !data.requestScopedAdmissible && (
        <p className="ui-callout ui-callout--attention" role="status">
          {t("request.funding.notAdmitted")}
        </p>
      )}

      <dl className="request-facts">
        <Fact label={t("request.funding.scope")}>
          {data.requestScoped
            ? t("request.funding.scopeComplete")
            : t("request.funding.scopeIncomplete")}
        </Fact>
        <Fact label={t("request.funding.intake")}>
          <code>{data.scope.intakeRequestId || t("common.notSet")}</code>
        </Fact>
        <Fact label={t("request.funding.lead")}>
          <code>{data.scope.leadRequestId || t("common.notSet")}</code>
        </Fact>
        <Fact label={t("request.funding.route")}>
          <code>
            {data.provider}/{data.model}
          </code>
        </Fact>
        <Fact label={t("request.funding.providerEvidence")}>
          {data.coreVerifiable
            ? t("common.unknown")
            : t("request.funding.providerEvidenceMissing")}
        </Fact>
        <Fact label={t("request.funding.guard")}>
          {data.requestScopedAdmissible
            ? t("request.funding.guardSatisfied")
            : t("request.funding.guardNotSatisfied")}
        </Fact>
      </dl>

      {data.admission ? (
        <section data-testid="funding-admission">
          <h3>{t("request.funding.admission")}</h3>
          <dl className="request-facts">
            <Fact label={t("request.funding.admissionId")}>
              <code>{data.admission.admissionId}</code>
            </Fact>
            <Fact label={t("request.funding.admissionState")}>
              <code>{data.admission.state}</code>
            </Fact>
            <Fact label={t("request.funding.route")}>
              <code>
                {data.admission.provider}/{data.admission.model}
              </code>
            </Fact>
            <Fact label={t("request.funding.settingsRevision")}>
              <code>{data.admission.settingsRevision}</code>
            </Fact>
            <Fact label={t("request.funding.validUntil")}>
              {formatDate(data.admission.validUntilMs)}
            </Fact>
            <Fact label={t("request.funding.extraUsage")}>
              <code>{data.admission.extraUsage}</code>
            </Fact>
            <Fact label={t("request.funding.confirmedBy")}>
              {data.admission.confirmedBy}
            </Fact>
            <Fact label={t("request.funding.campaign")}>
              {data.admission.campaignId ? (
                <code>{data.admission.campaignId}</code>
              ) : (
                t("common.notSet")
              )}
            </Fact>
          </dl>
          <p className="muted">{data.admission.statement}</p>
          <p className="muted">
            {t("request.funding.receipt")}:{" "}
            <code>{data.admission.receiptSha256}</code>
          </p>

          <h4>{t("request.funding.declarations")}</h4>
          {data.admission.ownerExtraUsageDeclarations.length === 0 ? (
            <p className="muted">{t("common.none")}</p>
          ) : (
            <div className="request-table-scroll">
              <table className="request-table">
                <thead>
                  <tr>
                    <th>{t("request.funding.table.declaration")}</th>
                    <th>{t("request.funding.table.role")}</th>
                    <th>{t("request.funding.table.provider")}</th>
                    <th>{t("request.funding.table.model")}</th>
                    <th>{t("request.funding.table.settingsRevision")}</th>
                  </tr>
                </thead>
                <tbody>
                  {data.admission.ownerExtraUsageDeclarations.map(
                    (declaration) => (
                      <tr key={declaration.declarationId}>
                        <td>
                          <code>{declaration.declarationId}</code>
                        </td>
                        <td>{declaration.role}</td>
                        <td>{declaration.provider}</td>
                        <td>{declaration.model}</td>
                        <td>{declaration.settingsRevision}</td>
                      </tr>
                    ),
                  )}
                </tbody>
              </table>
            </div>
          )}

          <h4>{t("request.funding.routes")}</h4>
          <div className="request-table-scroll">
            <table className="request-table">
              <thead>
                <tr>
                  <th>{t("request.funding.table.role")}</th>
                  <th>{t("request.funding.table.provider")}</th>
                  <th>{t("request.funding.table.model")}</th>
                  <th>{t("request.funding.table.settingsRevision")}</th>
                  <th>{t("request.funding.table.transport")}</th>
                  <th>{t("request.funding.table.valid")}</th>
                </tr>
              </thead>
              <tbody>
                {data.admission.routes.map((route) => (
                  <tr
                    key={`${route.role}:${route.provider}:${route.model}:${route.settingsRevision}:${route.transport}`}
                  >
                    <td>{route.role}</td>
                    <td>{route.provider}</td>
                    <td>{route.model}</td>
                    <td>{route.settingsRevision}</td>
                    <td>{route.transport}</td>
                    <td>{String(route.valid)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>
      ) : (
        <p className="muted">{t("request.funding.noAdmission")}</p>
      )}

      <h3>{t("request.funding.missingEvidence")}</h3>
      {data.missingEvidence.length === 0 ? (
        <p className="muted">{t("request.funding.noneMissing")}</p>
      ) : (
        <ValueList values={data.missingEvidence} />
      )}

      <h3>{t("request.funding.provenance")}</h3>
      <ValueList values={data.provenance} />

      <p className="muted">
        <strong>{t("request.funding.recovery")}:</strong> {data.recovery}
      </p>
      <p className="muted">{data.note}</p>
    </div>
  );
}
