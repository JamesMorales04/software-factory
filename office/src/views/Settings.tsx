import { useCallback, useEffect, useState } from "react";
import { Link, useSearchParams } from "react-router-dom";
import { StatusBadge } from "../components/office/StatusBadge";
import { Button } from "../components/ui/Button";
import { Checkbox, Input } from "../components/ui/Field";
import {
  EmptyState,
  ErrorState,
  LoadingState,
  PageHeader,
  Panel,
} from "../components/ui/Surface";
import { type TabItem, Tabs } from "../components/ui/Tabs";
import {
  CoreApiError,
  coreApi,
  type FactorySettings,
  type ProviderAudit,
  type ProviderModelCatalogEntry,
  type ProviderTestResult,
  type ProviderView,
} from "../coreApi";
import { formatTimestamp } from "../format";
import { LanguageSwitcher, useI18n } from "../i18n";
import { AccessDevices } from "./AccessDevices";
import { ExecutionProfiles } from "./ExecutionProfiles";

const TABS = ["profiles", "providers", "runtime", "access"] as const;
type SettingsTab = (typeof TABS)[number];

function resolveTab(value: string | null): SettingsTab {
  return TABS.includes(value as SettingsTab)
    ? (value as SettingsTab)
    : "providers";
}

function VerificationStep({
  label,
  status,
  detail,
}: {
  label: string;
  status: string;
  detail: string;
}) {
  return (
    <div className="verification-step">
      <span className="muted">{label}</span>
      <StatusBadge status={status} />
      <p className="muted">{detail}</p>
    </div>
  );
}

function ProviderCard({
  provider,
  onChanged,
}: {
  provider: ProviderView;
  onChanged: () => Promise<void>;
}) {
  const { t } = useI18n();
  const [model, setModel] = useState(provider.model);
  const [baseUrl, setBaseUrl] = useState(provider.baseUrl);
  const [credential, setCredential] = useState("");
  const [enabled, setEnabled] = useState(provider.enabled);
  const [isDefault, setIsDefault] = useState(provider.isDefault);
  const [maxTurns, setMaxTurns] = useState(provider.maxTurns);
  const [maxInput, setMaxInput] = useState(provider.maxInputTokens);
  const [maxOutput, setMaxOutput] = useState(provider.maxOutputTokens);
  const [maxCost, setMaxCost] = useState(provider.maxCostMicros);
  const [inputRate, setInputRate] = useState(provider.inputMicrosPerMillion);
  const [outputRate, setOutputRate] = useState(provider.outputMicrosPerMillion);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [test, setTest] = useState<ProviderTestResult | null>(null);
  const [audit, setAudit] = useState<ProviderAudit[]>([]);
  const [auditError, setAuditError] = useState<string | null>(null);
  const [revoking, setRevoking] = useState(false);
  const [catalog, setCatalog] = useState<ProviderModelCatalogEntry | null>(
    provider.modelDiscovery,
  );
  const [discoveryBusy, setDiscoveryBusy] = useState(false);
  const [discoveryError, setDiscoveryError] = useState<string | null>(null);

  useEffect(() => {
    setModel(provider.model);
    setBaseUrl(provider.baseUrl);
    setEnabled(provider.enabled);
    setIsDefault(provider.isDefault);
    setMaxTurns(provider.maxTurns);
    setMaxInput(provider.maxInputTokens);
    setMaxOutput(provider.maxOutputTokens);
    setMaxCost(provider.maxCostMicros);
    setInputRate(provider.inputMicrosPerMillion);
    setOutputRate(provider.outputMicrosPerMillion);
    setCatalog(provider.modelDiscovery);
  }, [provider]);

  const refreshAudit = useCallback(async () => {
    try {
      const result = await coreApi.providerAudit(provider.provider);
      setAudit(result.audit);
      setAuditError(null);
    } catch (cause) {
      setAuditError(
        cause instanceof CoreApiError
          ? cause.message
          : t("settings.provider.auditError"),
      );
    }
  }, [provider.provider, t]);

  useEffect(() => {
    void refreshAudit();
  }, [refreshAudit]);

  async function save(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await coreApi.updateProvider(provider.provider, {
        model: model.trim(),
        baseUrl: baseUrl.trim(),
        ...(credential === "" ? {} : { credential }),
        enabled,
        isDefault,
        maxTurns,
        maxInputTokens: maxInput,
        maxOutputTokens: maxOutput,
        maxCostMicros: maxCost,
        inputMicrosPerMillion: inputRate,
        outputMicrosPerMillion: outputRate,
      });
      setCredential("");
      setNotice(t("settings.provider.saved"));
      await onChanged();
      await refreshAudit();
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : t("settings.provider.saveFailed"),
      );
    } finally {
      setBusy(false);
    }
  }

  async function discoverModels() {
    setDiscoveryBusy(true);
    setDiscoveryError(null);
    try {
      const result = await coreApi.listProviderModels(provider.provider);
      setCatalog(result);
      await onChanged();
      await refreshAudit();
    } catch (cause) {
      setDiscoveryError(
        cause instanceof CoreApiError
          ? cause.message
          : t("settings.provider.discoveryFailed"),
      );
    } finally {
      setDiscoveryBusy(false);
    }
  }

  async function runTest() {
    setBusy(true);
    setError(null);
    setTest(null);
    try {
      setTest(await coreApi.testProvider(provider.provider));
      await refreshAudit();
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : t("settings.provider.testFailed"),
      );
    } finally {
      setBusy(false);
    }
  }

  async function revoke() {
    setBusy(true);
    setError(null);
    try {
      await coreApi.revokeProviderCredential(provider.provider);
      setNotice(t("settings.provider.revoked"));
      setCredential("");
      setRevoking(false);
      await onChanged();
      await refreshAudit();
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : t("settings.provider.revokeFailed"),
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <Panel className="settings-provider" data-testid="provider-card">
      <div className="settings-provider-head">
        <div>
          <h2>{provider.displayName}</h2>
          <p className="muted">
            <code>{provider.provider}</code> / {provider.kind} · {provider.note}
          </p>
        </div>
        <div className="settings-badges">
          <StatusBadge
            status={provider.configured ? "CONFIGURED" : "NOT_CONFIGURED"}
            label={
              provider.configured
                ? t("settings.provider.credentialConfigured")
                : t("settings.provider.notConfigured")
            }
          />
          <StatusBadge
            status={provider.enabled ? "ENABLED" : "DISABLED"}
            label={
              provider.enabled
                ? t("settings.provider.enabled")
                : t("settings.provider.disabled")
            }
          />
          {provider.isDefault && (
            <StatusBadge
              status="DEFAULT"
              label={t("settings.provider.defaultRoute")}
            />
          )}
          <StatusBadge status={provider.status} />
        </div>
      </div>

      <dl className="overview-facts">
        <div>
          <dt>{t("settings.provider.model")}</dt>
          <dd>{provider.model}</dd>
        </div>
        <div>
          <dt>{t("settings.provider.endpoint")}</dt>
          <dd>
            <code>{provider.baseUrl}</code>
          </dd>
        </div>
        <div>
          <dt>{t("settings.provider.authModes")}</dt>
          <dd>{provider.authModes.join(", ")}</dd>
        </div>
        <div>
          <dt>{t("settings.provider.chatgptOauth")}</dt>
          <dd>
            <StatusBadge status={provider.chatgptOauth} />
          </dd>
        </div>
        <div>
          <dt>{t("settings.provider.revision")}</dt>
          <dd>{provider.revision}</dd>
        </div>
        <div>
          <dt>{t("settings.provider.updated")}</dt>
          <dd>{formatTimestamp(provider.updatedAt)}</dd>
        </div>
      </dl>

      <section
        className="verification-path"
        aria-label={t("settings.provider.verification.aria")}
      >
        <VerificationStep
          label={t("settings.provider.verification.route")}
          status={provider.configured ? "CONFIGURED" : "NOT_CONFIGURED"}
          detail={t("settings.provider.verification.routeDetail")}
        />
        <VerificationStep
          label={t("settings.provider.verification.test")}
          status={test?.status ?? "NOT_TESTED"}
          detail={
            test
              ? t("settings.provider.verification.testDetail", {
                  detail: test.detail,
                  model: test.modelAvailable
                    ? t("settings.provider.listed")
                    : t("settings.provider.notListed"),
                })
              : t("settings.provider.verification.testNotRun")
          }
        />
        <VerificationStep
          label={t("settings.provider.verification.inference")}
          status="NOT_TESTED"
          detail={t("settings.provider.verification.inferenceDetail")}
        />
      </section>

      <section aria-label={t("settings.provider.catalog.aria")}>
        <div className="section-heading">
          <div>
            <h3>{t("settings.provider.catalog.title")}</h3>
            <span className="muted">
              {t("settings.provider.catalog.description")}
            </span>
          </div>
          <Button
            type="button"
            size="sm"
            disabled={busy || discoveryBusy || !provider.configured}
            onClick={() => void discoverModels()}
          >
            {discoveryBusy
              ? t("settings.provider.catalog.discovering")
              : t("settings.provider.catalog.discover")}
          </Button>
        </div>
        {discoveryError && (
          <p className="error" role="alert">
            {discoveryError}
          </p>
        )}
        {catalog === null ? (
          <p className="muted">
            {provider.configured
              ? t("settings.provider.catalog.emptyConfigured")
              : t("settings.provider.catalog.emptyUnconfigured")}
          </p>
        ) : (
          <div>
            <p>
              <StatusBadge status={catalog.status} />{" "}
              <span className="muted">
                {catalog.detail} · {formatTimestamp(catalog.discoveredAt)}
              </span>
            </p>
            {catalog.status === "AVAILABLE" && catalog.models.length > 0 ? (
              <ul className="settings-model-catalog">
                {catalog.models.map((discovered) => (
                  <li key={discovered}>
                    <code>{discovered}</code>
                    <Button
                      type="button"
                      size="sm"
                      onClick={() => setModel(discovered)}
                    >
                      {t("settings.provider.catalog.use")}
                    </Button>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="muted">{t("settings.provider.catalog.noModels")}</p>
            )}
          </div>
        )}
      </section>

      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      {notice && <p className="notice">{notice}</p>}

      <form className="settings-form" onSubmit={(event) => void save(event)}>
        <div className="grid-2">
          <Input
            label={t("settings.provider.model")}
            value={model}
            onChange={(event) => setModel(event.target.value)}
            required
          />
          <Input
            label={t("settings.provider.baseUrl")}
            value={baseUrl}
            onChange={(event) => setBaseUrl(event.target.value)}
            required
          />
        </div>
        <Input
          label={
            provider.configured
              ? t("settings.provider.replaceCredential")
              : t("settings.provider.credential")
          }
          type="password"
          value={credential}
          onChange={(event) => setCredential(event.target.value)}
          autoComplete="new-password"
          required={!provider.configured}
          placeholder={
            provider.configured
              ? t("settings.provider.credentialUnchanged")
              : t("settings.provider.credentialPlaceholder")
          }
          help={t("settings.provider.credentialHelp")}
        />
        <details className="settings-advanced">
          <summary>{t("settings.provider.advanced")}</summary>
          <div className="grid-2">
            <Input
              label={t("settings.provider.maxTurns")}
              type="number"
              min={0}
              value={maxTurns}
              onChange={(event) => setMaxTurns(Number(event.target.value))}
            />
            <Input
              label={t("settings.provider.maxCost")}
              type="number"
              min={0}
              value={maxCost}
              onChange={(event) => setMaxCost(Number(event.target.value))}
            />
            <Input
              label={t("settings.provider.maxInput")}
              type="number"
              min={0}
              value={maxInput}
              onChange={(event) => setMaxInput(Number(event.target.value))}
            />
            <Input
              label={t("settings.provider.maxOutput")}
              type="number"
              min={0}
              value={maxOutput}
              onChange={(event) => setMaxOutput(Number(event.target.value))}
            />
            <Input
              label={t("settings.provider.inputRate")}
              type="number"
              min={0}
              value={inputRate}
              onChange={(event) => setInputRate(Number(event.target.value))}
            />
            <Input
              label={t("settings.provider.outputRate")}
              type="number"
              min={0}
              value={outputRate}
              onChange={(event) => setOutputRate(Number(event.target.value))}
            />
          </div>
          <p className="muted">{t("settings.provider.advancedHint")}</p>
        </details>
        <div className="settings-checks">
          <Checkbox
            label={t("settings.provider.enabledLabel")}
            checked={enabled}
            onChange={(event) => setEnabled(event.target.checked)}
          />
          <Checkbox
            label={t("settings.provider.defaultLabel")}
            checked={isDefault}
            onChange={(event) => setIsDefault(event.target.checked)}
          />
        </div>
        <div className="row">
          <Button type="submit" variant="primary" disabled={busy}>
            {busy ? t("settings.provider.saving") : t("settings.provider.save")}
          </Button>
          <Button
            type="button"
            disabled={busy || !provider.configured}
            onClick={() => void runTest()}
          >
            {busy
              ? t("settings.provider.working")
              : t("settings.provider.test")}
          </Button>
          <Button
            type="button"
            variant="danger"
            disabled={busy || !provider.configured}
            onClick={() => setRevoking(true)}
          >
            {t("settings.provider.revoke")}
          </Button>
        </div>
      </form>

      {revoking && (
        <div className="ui-callout ui-callout--danger" role="alert">
          <p>
            {t("settings.provider.revokeConfirm", {
              provider: provider.displayName,
            })}
          </p>
          <div className="row">
            <Button
              variant="danger"
              disabled={busy}
              onClick={() => void revoke()}
            >
              {busy
                ? t("settings.provider.revoking")
                : t("settings.provider.confirmRevoke")}
            </Button>
            <Button
              type="button"
              disabled={busy}
              onClick={() => setRevoking(false)}
            >
              {t("settings.provider.keepCredential")}
            </Button>
          </div>
        </div>
      )}

      {test && (
        <div className="ui-callout ui-callout--info" role="status">
          <p>
            {t("settings.provider.testTitle")}{" "}
            <StatusBadge status={test.status} /> · {test.detail}
            {" · "}
            {t("settings.provider.testModel")} <code>{test.model}</code>{" "}
            {test.modelAvailable
              ? t("settings.provider.listed")
              : t("settings.provider.notListed")}
          </p>
          {test.models.length > 0 && (
            <p className="muted">
              {t("settings.provider.models", {
                models: `${test.models.slice(0, 20).join(", ")}${
                  test.models.length > 20 ? "…" : ""
                }`,
              })}
            </p>
          )}
          <p className="muted">{t("settings.provider.testNote")}</p>
        </div>
      )}

      <details className="settings-audit">
        <summary>
          {t("settings.provider.audit.title", { count: audit.length })}
        </summary>
        {auditError ? (
          <div className="ui-callout ui-callout--danger" role="alert">
            <p>{t("settings.provider.audit.error", { error: auditError })}</p>
            <Button size="sm" onClick={() => void refreshAudit()}>
              {t("settings.provider.audit.retry")}
            </Button>
          </div>
        ) : audit.length === 0 ? (
          <EmptyState
            title={t("settings.provider.audit.emptyTitle")}
            description={t("settings.provider.audit.emptyDescription")}
          />
        ) : (
          <table>
            <thead>
              <tr>
                <th>{t("settings.provider.audit.revision")}</th>
                <th>{t("settings.provider.audit.action")}</th>
                <th>{t("settings.provider.audit.actor")}</th>
                <th>{t("settings.provider.audit.detail")}</th>
                <th>{t("settings.provider.audit.time")}</th>
              </tr>
            </thead>
            <tbody>
              {audit.map((entry) => (
                <tr key={entry.auditId}>
                  <td>{entry.revision}</td>
                  <td>{entry.action}</td>
                  <td>{entry.actor}</td>
                  <td>{entry.detail}</td>
                  <td className="muted">{formatTimestamp(entry.createdAt)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </details>
    </Panel>
  );
}

export function Settings() {
  const { t } = useI18n();
  const [searchParams, setSearchParams] = useSearchParams();
  const tab = resolveTab(searchParams.get("tab"));
  const needsProviderSettings = tab === "providers" || tab === "runtime";
  const [providers, setProviders] = useState<ProviderView[]>([]);
  const [runtime, setRuntime] = useState<FactorySettings | null>(null);
  const [reviewEnabled, setReviewEnabled] = useState(false);
  const [reviewTokens, setReviewTokens] = useState(8192);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [lastLoadedAt, setLastLoadedAt] = useState<string | null>(null);

  const selectTab = (next: string) => {
    const params = new URLSearchParams(searchParams);
    params.set("tab", resolveTab(next));
    setSearchParams(params);
  };

  const load = useCallback(async () => {
    try {
      const [providerResponse, runtimeSettings] = await Promise.all([
        coreApi.listProviders(),
        coreApi.getRuntimeSettings(),
      ]);
      setProviders(providerResponse.providers);
      setRuntime(runtimeSettings);
      setReviewEnabled(runtimeSettings.semanticReviewEnabled);
      setReviewTokens(runtimeSettings.semanticReviewMaxOutputTokens);
      setLastLoadedAt(new Date().toISOString());
      setLoaded(true);
      setError(null);
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : t("settings.error.load"),
      );
    }
  }, [t]);

  useEffect(() => {
    if (needsProviderSettings) void load();
  }, [load, needsProviderSettings]);

  async function saveRuntime(event: React.FormEvent) {
    event.preventDefault();
    setNotice(null);
    setError(null);
    try {
      const next = await coreApi.updateRuntimeSettings({
        semanticReviewEnabled: reviewEnabled,
        semanticReviewMaxOutputTokens: reviewTokens,
      });
      setRuntime(next);
      setNotice(t("settings.runtime.saved"));
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : t("settings.runtime.saveFailed"),
      );
    }
  }

  if (needsProviderSettings && error && !loaded) {
    return (
      <ErrorState
        message={error}
        action={
          <Button onClick={() => void load()}>{t("common.retry")}</Button>
        }
      />
    );
  }
  if (needsProviderSettings && !loaded)
    return <LoadingState label={t("settings.loading")} />;

  const tabs: TabItem[] = [
    { id: "profiles", label: t("settings.tabs.profiles") },
    {
      id: "providers",
      label: t("settings.tabs.providers"),
      count: providers.length,
    },
    { id: "runtime", label: t("settings.tabs.runtime") },
    { id: "access", label: t("settings.tabs.access") },
  ];

  return (
    <div className="settings-workspace">
      <PageHeader
        title={t("settings.title")}
        description={
          tab === "access"
            ? t("settings.description.access")
            : tab === "profiles"
              ? t("settings.description.profiles")
              : t("settings.description.providers")
        }
        meta={
          <>
            {needsProviderSettings && (
              <span>
                {t("settings.meta.providers", { count: providers.length })}
              </span>
            )}
            {needsProviderSettings && (
              <span>
                {t("settings.meta.fetched", {
                  date: lastLoadedAt ? formatTimestamp(lastLoadedAt) : "—",
                })}
              </span>
            )}
          </>
        }
        actions={
          needsProviderSettings ? (
            <Button size="sm" onClick={() => void load()}>
              {t("settings.refresh")}
            </Button>
          ) : undefined
        }
      />
      <LanguageSwitcher />
      {needsProviderSettings && error && (
        <div className="ui-callout ui-callout--danger" role="alert">
          {error} {t("settings.error.stale")}
        </div>
      )}
      {notice && <p className="notice">{notice}</p>}
      <Tabs
        items={tabs}
        value={tab}
        onChange={selectTab}
        label={t("settings.tabs.label")}
      />

      {tab === "profiles" && (
        <div
          id="panel-profiles"
          role="tabpanel"
          aria-labelledby="tab-profiles"
          className="campaign-tab-panel"
        >
          <ExecutionProfiles />
        </div>
      )}

      {tab === "providers" && (
        <div
          id="panel-providers"
          role="tabpanel"
          aria-labelledby="tab-providers"
          className="campaign-tab-panel"
        >
          {providers.length === 0 ? (
            <EmptyState
              title={t("settings.providers.emptyTitle")}
              description={t("settings.providers.emptyDescription")}
            />
          ) : (
            providers.map((provider) => (
              <ProviderCard
                key={provider.provider}
                provider={provider}
                onChanged={load}
              />
            ))
          )}
        </div>
      )}

      {tab === "runtime" && (
        <div
          id="panel-runtime"
          role="tabpanel"
          aria-labelledby="tab-runtime"
          className="campaign-tab-panel"
        >
          <Panel>
            <div className="section-heading">
              <h2>{t("settings.runtime.title")}</h2>
              <span className="muted">{t("settings.runtime.hint")}</span>
            </div>
            <p className="muted">{t("settings.runtime.description")}</p>
            <form
              className="settings-form"
              onSubmit={(event) => void saveRuntime(event)}
            >
              <Checkbox
                label={t("settings.runtime.enable")}
                checked={reviewEnabled}
                onChange={(event) => setReviewEnabled(event.target.checked)}
              />
              <Input
                label={t("settings.runtime.maxOutput")}
                type="number"
                min={128}
                max={32768}
                value={reviewTokens}
                onChange={(event) =>
                  setReviewTokens(Number(event.target.value))
                }
              />
              <p className="muted">
                {runtime
                  ? t("settings.runtime.current", {
                      mechanism: runtime.semanticReviewEnabled
                        ? "MODEL"
                        : "DETERMINISTIC",
                      date: formatTimestamp(runtime.updatedAt),
                    })
                  : t("settings.runtime.notLoaded")}
              </p>
              <div className="row">
                <Button type="submit" variant="primary">
                  {t("settings.runtime.save")}
                </Button>
              </div>
            </form>
          </Panel>
          <Panel>
            <div className="section-heading">
              <h2>{t("settings.boundaries.title")}</h2>
              <Link to="/runtime">{t("settings.boundaries.openRuntime")}</Link>
            </div>
            <dl className="detail">
              <dt>{t("settings.boundaries.credentials")}</dt>
              <dd>{t("settings.boundaries.credentialsDetail")}</dd>
              <dt>{t("settings.boundaries.frozen")}</dt>
              <dd>{t("settings.boundaries.frozenDetail")}</dd>
              <dt>{t("settings.boundaries.test")}</dt>
              <dd>{t("settings.boundaries.testDetail")}</dd>
              <dt>{t("settings.boundaries.oauth")}</dt>
              <dd>{t("settings.boundaries.oauthDetail")}</dd>
              <dt>{t("settings.boundaries.fingerprint")}</dt>
              <dd>{t("settings.boundaries.fingerprintDetail")}</dd>
            </dl>
          </Panel>
        </div>
      )}

      {tab === "access" && (
        <div
          id="panel-access"
          role="tabpanel"
          aria-labelledby="tab-access"
          className="campaign-tab-panel"
        >
          <AccessDevices />
        </div>
      )}
    </div>
  );
}
