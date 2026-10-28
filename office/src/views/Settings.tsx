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
  type ProviderTestResult,
  type ProviderView,
} from "../coreApi";
import { formatTimestamp } from "../format";
import { LanguageSwitcher } from "../i18n";
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
          : "Configuration audit could not be loaded.",
      );
    }
  }, [provider.provider]);

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
      setNotice(
        "Configuration saved. The credential was not returned to this page.",
      );
      await onChanged();
      await refreshAudit();
    } catch (cause) {
      setError(
        cause instanceof CoreApiError ? cause.message : "Provider save failed.",
      );
    } finally {
      setBusy(false);
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
          : "Connection test failed.",
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
      setNotice("Credential revoked and provider disabled.");
      setCredential("");
      setRevoking(false);
      await onChanged();
      await refreshAudit();
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : "Credential revocation failed.",
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
              provider.configured ? "credential configured" : "not configured"
            }
          />
          <StatusBadge
            status={provider.enabled ? "ENABLED" : "DISABLED"}
            label={provider.enabled ? "enabled" : "disabled"}
          />
          {provider.isDefault && (
            <StatusBadge status="DEFAULT" label="default route" />
          )}
          <StatusBadge status={provider.status} />
        </div>
      </div>

      <dl className="overview-facts">
        <div>
          <dt>Model</dt>
          <dd>{provider.model}</dd>
        </div>
        <div>
          <dt>Endpoint</dt>
          <dd>
            <code>{provider.baseUrl}</code>
          </dd>
        </div>
        <div>
          <dt>Auth modes</dt>
          <dd>{provider.authModes.join(", ")}</dd>
        </div>
        <div>
          <dt>ChatGPT OAuth</dt>
          <dd>
            <StatusBadge status={provider.chatgptOauth} />
          </dd>
        </div>
        <div>
          <dt>Revision</dt>
          <dd>{provider.revision}</dd>
        </div>
        <div>
          <dt>Updated</dt>
          <dd>{formatTimestamp(provider.updatedAt)}</dd>
        </div>
      </dl>

      <section className="verification-path" aria-label="Verification path">
        <VerificationStep
          label="Configured route"
          status={provider.configured ? "CONFIGURED" : "NOT_CONFIGURED"}
          detail="A saved route and credential are configuration, not proof of access."
        />
        <VerificationStep
          label="Connection test"
          status={test?.status ?? "NOT_TESTED"}
          detail={
            test
              ? `${test.detail}; model ${test.modelAvailable ? "listed" : "not listed"}`
              : "Not run in this page load. A model list is not coding inference."
          }
        />
        <VerificationStep
          label="Coding inference"
          status="NOT_TESTED"
          detail="No paid inference is authorized or proven by a /models response."
        />
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
            label="Model"
            value={model}
            onChange={(event) => setModel(event.target.value)}
            required
          />
          <Input
            label="Base URL"
            value={baseUrl}
            onChange={(event) => setBaseUrl(event.target.value)}
            required
          />
        </div>
        <Input
          label={
            provider.configured
              ? "Replace credential (leave blank to keep current)"
              : "Provider credential"
          }
          type="password"
          value={credential}
          onChange={(event) => setCredential(event.target.value)}
          autoComplete="new-password"
          required={!provider.configured}
          placeholder={provider.configured ? "unchanged" : "provider API key"}
          help="Credentials are encrypted by Core, never returned to the browser and never rendered in audit rows."
        />
        <details className="settings-advanced">
          <summary>Advanced budget and rate limits</summary>
          <div className="grid-2">
            <Input
              label="Max turns"
              type="number"
              min={0}
              value={maxTurns}
              onChange={(event) => setMaxTurns(Number(event.target.value))}
            />
            <Input
              label="Max cost (micros)"
              type="number"
              min={0}
              value={maxCost}
              onChange={(event) => setMaxCost(Number(event.target.value))}
            />
            <Input
              label="Max input tokens"
              type="number"
              min={0}
              value={maxInput}
              onChange={(event) => setMaxInput(Number(event.target.value))}
            />
            <Input
              label="Max output tokens"
              type="number"
              min={0}
              value={maxOutput}
              onChange={(event) => setMaxOutput(Number(event.target.value))}
            />
            <Input
              label="Input micros / million"
              type="number"
              min={0}
              value={inputRate}
              onChange={(event) => setInputRate(Number(event.target.value))}
            />
            <Input
              label="Output micros / million"
              type="number"
              min={0}
              value={outputRate}
              onChange={(event) => setOutputRate(Number(event.target.value))}
            />
          </div>
          <p className="muted">
            These are provider-route defaults and Core model-call caps. The
            explicit Campaign authorization remains authoritative for provider
            sessions; these limits do not widen it. Frozen Campaign bindings are
            not rewritten by a provider settings revision.
          </p>
        </details>
        <div className="settings-checks">
          <Checkbox
            label="Enabled"
            checked={enabled}
            onChange={(event) => setEnabled(event.target.checked)}
          />
          <Checkbox
            label="Default route for Core model calls"
            checked={isDefault}
            onChange={(event) => setIsDefault(event.target.checked)}
          />
        </div>
        <div className="row">
          <Button type="submit" variant="primary" disabled={busy}>
            {busy ? "Saving…" : "Save configuration"}
          </Button>
          <Button
            type="button"
            disabled={busy || !provider.configured}
            onClick={() => void runTest()}
          >
            {busy ? "Working…" : "Test connection"}
          </Button>
          <Button
            type="button"
            variant="danger"
            disabled={busy || !provider.configured}
            onClick={() => setRevoking(true)}
          >
            Revoke credential
          </Button>
        </div>
      </form>

      {revoking && (
        <div className="ui-callout ui-callout--danger" role="alert">
          <p>
            Revoke the stored credential for{" "}
            <strong>{provider.displayName}</strong>? Core will disable this
            provider route. Already frozen Campaign bindings keep their recorded
            route; no new Core model call can use the credential.
          </p>
          <div className="row">
            <Button
              variant="danger"
              disabled={busy}
              onClick={() => void revoke()}
            >
              {busy ? "Revoking…" : "Confirm revocation"}
            </Button>
            <Button
              type="button"
              disabled={busy}
              onClick={() => setRevoking(false)}
            >
              Keep credential
            </Button>
          </div>
        </div>
      )}

      {test && (
        <div className="ui-callout ui-callout--info" role="status">
          <p>
            Connection test: <StatusBadge status={test.status} /> ·{" "}
            {test.detail}
            {" · "}model <code>{test.model}</code>{" "}
            {test.modelAvailable ? "listed" : "not listed"}
          </p>
          {test.models.length > 0 && (
            <p className="muted">
              Models: {test.models.slice(0, 20).join(", ")}
              {test.models.length > 20 ? "…" : ""}
            </p>
          )}
          <p className="muted">
            A model list does not certify access, pricing or coding inference.
          </p>
        </div>
      )}

      <details className="settings-audit">
        <summary>Configuration audit ({audit.length})</summary>
        {auditError ? (
          <div className="ui-callout ui-callout--danger" role="alert">
            <p>{auditError} The audit history is unknown, not empty.</p>
            <Button size="sm" onClick={() => void refreshAudit()}>
              Retry audit
            </Button>
          </div>
        ) : audit.length === 0 ? (
          <EmptyState
            title="No audit entries"
            description="Core returned an empty audit history for this provider."
          />
        ) : (
          <table>
            <thead>
              <tr>
                <th>Revision</th>
                <th>Action</th>
                <th>Actor</th>
                <th>Detail</th>
                <th>Time</th>
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
          : "Failed to load provider settings.",
      );
    }
  }, []);

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
      setNotice("Runtime review policy saved.");
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : "Runtime policy update failed.",
      );
    }
  }

  if (needsProviderSettings && error && !loaded) {
    return (
      <ErrorState
        message={error}
        action={<Button onClick={() => void load()}>Retry</Button>}
      />
    );
  }
  if (needsProviderSettings && !loaded)
    return <LoadingState label="Loading settings…" />;

  const tabs: TabItem[] = [
    { id: "profiles", label: "Execution profiles" },
    { id: "providers", label: "Providers & models", count: providers.length },
    { id: "runtime", label: "Runtime policy" },
    { id: "access", label: "Access & Devices" },
  ];

  return (
    <div className="settings-workspace">
      <PageHeader
        title="Settings"
        description={
          tab === "access"
            ? "Review local and private remote access, pair trusted devices, and revoke sessions."
            : tab === "profiles"
              ? "Create versioned, Core-owned execution profiles for the Lead, Implementer and Semantic Reviewer roles. A profile is configuration, not an authorization."
              : "Configure Core-owned provider routes and review policy. Credentials are encrypted at rest and never returned after submission. A configured route is not proof of live coding inference."
        }
        meta={
          <>
            {needsProviderSettings && (
              <span>{providers.length} provider(s)</span>
            )}
            {needsProviderSettings && (
              <span>
                Fetched {lastLoadedAt ? formatTimestamp(lastLoadedAt) : "—"}
              </span>
            )}
          </>
        }
        actions={
          needsProviderSettings ? (
            <Button size="sm" onClick={() => void load()}>
              Refresh
            </Button>
          ) : undefined
        }
      />
      <LanguageSwitcher />
      {needsProviderSettings && error && (
        <div className="ui-callout ui-callout--danger" role="alert">
          {error} The displayed configuration may be stale.
        </div>
      )}
      {notice && <p className="notice">{notice}</p>}
      <Tabs
        items={tabs}
        value={tab}
        onChange={selectTab}
        label="Settings sections"
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
              title="No provider routes"
              description="Core did not return a provider catalog entry."
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
              <h2>Review and runtime policy</h2>
              <span className="muted">
                This policy is separate from a provider route or Campaign
                authorization
              </span>
            </div>
            <p className="muted">
              Semantic Reviewer model calls are disabled by default. Enabling
              them authorizes bounded Core-owned model calls for each candidate
              review; they may incur provider charges. The verifier always
              executes the frozen checks independently.
            </p>
            <form
              className="settings-form"
              onSubmit={(event) => void saveRuntime(event)}
            >
              <Checkbox
                label="Enable semantic Reviewer model calls (paid provider route)"
                checked={reviewEnabled}
                onChange={(event) => setReviewEnabled(event.target.checked)}
              />
              <Input
                label="Semantic review max output tokens"
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
                  ? `Current: ${runtime.semanticReviewEnabled ? "MODEL" : "DETERMINISTIC"} · updated ${formatTimestamp(runtime.updatedAt)}`
                  : "Runtime policy not loaded."}
              </p>
              <div className="row">
                <Button type="submit" variant="primary">
                  Save runtime policy
                </Button>
              </div>
            </form>
          </Panel>
          <Panel>
            <div className="section-heading">
              <h2>Configuration boundaries</h2>
              <Link to="/runtime">Open Runtime diagnostics</Link>
            </div>
            <dl className="detail">
              <dt>Provider credentials</dt>
              <dd>Encrypted by Core; never returned to this browser.</dd>
              <dt>Frozen Campaigns</dt>
              <dd>
                Provider revisions do not rewrite an already frozen Campaign or
                Delegation binding.
              </dd>
              <dt>Connection test</dt>
              <dd>
                Reads the provider model list only. It is not coding inference.
              </dd>
              <dt>ChatGPT OAuth</dt>
              <dd>
                Unsupported provider OAuth states remain visible as blocked or
                not applicable; Office does not claim live Codex authentication.
              </dd>
              <dt>Credential fingerprint</dt>
              <dd>
                Configuration rows carry a non-secret fingerprint; the
                credential value is never rendered.
              </dd>
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
