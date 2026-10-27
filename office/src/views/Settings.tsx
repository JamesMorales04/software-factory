import { useCallback, useEffect, useState } from "react";
import {
  CoreApiError,
  coreApi,
  type FactorySettings,
  type ProviderAudit,
  type ProviderTestResult,
  type ProviderView,
} from "../coreApi";
import { formatTimestamp } from "../format";

function numberField(
  label: string,
  value: number,
  onChange: (value: number) => void,
  help?: string,
) {
  return (
    <label>
      {label}
      <input
        type="number"
        min={0}
        value={value}
        onChange={(event) => onChange(Number(event.target.value))}
      />
      {help && <span className="muted">{help}</span>}
    </label>
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
      setAudit((await coreApi.providerAudit(provider.provider)).audit);
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
      setAudit((await coreApi.providerAudit(provider.provider)).audit);
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
    if (!window.confirm(`Revoke the ${provider.displayName} credential?`)) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await coreApi.revokeProviderCredential(provider.provider);
      setNotice("Credential revoked and provider disabled.");
      setCredential("");
      await onChanged();
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
    <section className="card">
      <div className="row">
        <div>
          <h3>{provider.displayName}</h3>
          <p className="muted">
            <code>{provider.provider}</code> / {provider.kind} · {provider.note}
          </p>
        </div>
        <div>
          <span
            className={`pill ${provider.configured ? "accepted" : "blocked"}`}
          >
            {provider.configured ? "credential configured" : "not configured"}
          </span>{" "}
          <span
            className={`pill ${provider.enabled ? "accepted" : "not-tested"}`}
          >
            {provider.enabled ? "enabled" : "disabled"}
          </span>{" "}
          {provider.isDefault && <span className="pill accepted">default</span>}
        </div>
      </div>
      <p className="muted">
        ChatGPT OAuth: <strong>{provider.chatgptOauth}</strong> · auth:{" "}
        {provider.authModes.join(", ")}
      </p>
      {error && <p className="error">{error}</p>}
      {notice && <p className="notice">{notice}</p>}
      <form className="create" onSubmit={(event) => void save(event)}>
        <div className="grid-2">
          <label>
            Model
            <input
              value={model}
              onChange={(event) => setModel(event.target.value)}
              required
            />
          </label>
          <label>
            Base URL
            <input
              value={baseUrl}
              onChange={(event) => setBaseUrl(event.target.value)}
              required
            />
          </label>
        </div>
        <label>
          Credential{" "}
          {provider.configured ? "(leave blank to keep the current value)" : ""}
          <input
            type="password"
            value={credential}
            onChange={(event) => setCredential(event.target.value)}
            autoComplete="new-password"
            placeholder={
              provider.configured ? "•••••••• unchanged" : "provider API key"
            }
          />
        </label>
        <div className="grid-2">
          {numberField("Max turns", maxTurns, setMaxTurns)}
          {numberField("Max cost (micros)", maxCost, setMaxCost)}
          {numberField("Max input tokens", maxInput, setMaxInput)}
          {numberField("Max output tokens", maxOutput, setMaxOutput)}
          {numberField("Input micros / million", inputRate, setInputRate)}
          {numberField("Output micros / million", outputRate, setOutputRate)}
        </div>
        <p className="muted">
          These are provider-route defaults and cap Core model calls. The
          explicit Campaign authorization remains authoritative for provider
          sessions; provider limits do not widen it.
        </p>
        <label className="inline">
          <input
            type="checkbox"
            checked={enabled}
            onChange={(event) => setEnabled(event.target.checked)}
          />
          Enabled
        </label>
        <label className="inline">
          <input
            type="checkbox"
            checked={isDefault}
            onChange={(event) => setIsDefault(event.target.checked)}
          />
          Default route for Core model calls
        </label>
        <div className="row">
          <button type="submit" disabled={busy}>
            {busy ? "Saving…" : "Save configuration"}
          </button>
          <button
            type="button"
            disabled={busy || !provider.configured}
            onClick={() => void runTest()}
          >
            Test connection
          </button>
          <button
            type="button"
            className="danger"
            disabled={busy || !provider.configured}
            onClick={() => void revoke()}
          >
            Revoke credential
          </button>
        </div>
      </form>
      {test && (
        <div className="notice">
          <p>
            Test: <strong>{test.status}</strong> · {test.detail} · model{" "}
            <code>{test.model}</code>{" "}
            {test.modelAvailable ? "listed" : "not listed"}
          </p>
          {test.models.length > 0 && (
            <p className="muted">
              Models: {test.models.slice(0, 20).join(", ")}
              {test.models.length > 20 ? "…" : ""}
            </p>
          )}
          <p className="muted">
            A model list does not certify access, pricing or inference
            capability.
          </p>
        </div>
      )}
      {audit.length > 0 && (
        <details>
          <summary>Configuration audit ({audit.length})</summary>
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
        </details>
      )}
    </section>
  );
}

export function Settings() {
  const [providers, setProviders] = useState<ProviderView[]>([]);
  const [runtime, setRuntime] = useState<FactorySettings | null>(null);
  const [reviewEnabled, setReviewEnabled] = useState(false);
  const [reviewTokens, setReviewTokens] = useState(8192);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

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
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : "Failed to load providers.",
      );
    }
  }, []);

  useEffect(() => {
    void load();
    const timer = setInterval(() => void load(), 5000);
    return () => clearInterval(timer);
  }, [load]);

  return (
    <div>
      <h2>Settings — Providers &amp; Models</h2>
      <p className="muted">
        Configure Core-owned provider routes. Credentials are encrypted at rest,
        never returned after submission and never written to logs. Saving a
        configuration does not start inference. Testing reads the provider model
        list only.
      </p>
      {error && <p className="error">{error}</p>}
      {notice && <p className="notice">{notice}</p>}
      <section className="card">
        <h3>Review &amp; runtime policy</h3>
        <p className="muted">
          Semantic Reviewer model calls are disabled by default. Enabling them
          authorizes bounded Core-owned model calls for each candidate review;
          they may incur provider charges. The verifier always executes the
          frozen checks independently.
        </p>
        <form
          className="create"
          onSubmit={(event) => {
            event.preventDefault();
            void coreApi
              .updateRuntimeSettings({
                semanticReviewEnabled: reviewEnabled,
                semanticReviewMaxOutputTokens: reviewTokens,
              })
              .then((next) => {
                setRuntime(next);
                setNotice("Runtime review policy saved.");
              })
              .catch((cause) =>
                setError(
                  cause instanceof CoreApiError
                    ? cause.message
                    : "Runtime policy update failed.",
                ),
              );
          }}
        >
          <label className="inline">
            <input
              type="checkbox"
              checked={reviewEnabled}
              onChange={(event) => setReviewEnabled(event.target.checked)}
            />
            Enable semantic Reviewer model calls (paid provider route)
          </label>
          <label>
            Semantic review max output tokens
            <input
              type="number"
              min={128}
              max={32768}
              value={reviewTokens}
              onChange={(event) => setReviewTokens(Number(event.target.value))}
            />
          </label>
          <p className="muted">
            {runtime
              ? `Current: ${runtime.semanticReviewEnabled ? "MODEL" : "DETERMINISTIC"} · updated ${formatTimestamp(runtime.updatedAt)}`
              : "Loading runtime policy…"}
          </p>
          <button type="submit">Save runtime policy</button>
        </form>
      </section>
      {providers.map((provider) => (
        <ProviderCard
          key={provider.provider}
          provider={provider}
          onChanged={load}
        />
      ))}
    </div>
  );
}
