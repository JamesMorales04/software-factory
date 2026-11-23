import { useEffect, useState } from "react";
import { Badge } from "../components/ui/Badge";
import { coreApi, type SubscriptionFundingStatus } from "../coreApi";
import { useI18n } from "../i18n";

const SUBSCRIPTION_PROVIDERS = new Set(["opencode-go", "codex"]);

export function ProviderFundingStatus({ provider }: { provider: string }) {
  const { t } = useI18n();
  const [status, setStatus] = useState<SubscriptionFundingStatus | null>(null);
  const [unavailable, setUnavailable] = useState(false);

  useEffect(() => {
    if (!SUBSCRIPTION_PROVIDERS.has(provider)) return;
    let active = true;
    setStatus(null);
    setUnavailable(false);
    coreApi
      .getSubscriptionFunding(provider)
      .then((next) => {
        if (active) setStatus(next);
      })
      .catch(() => {
        if (active) setUnavailable(true);
      });
    return () => {
      active = false;
    };
  }, [provider]);

  if (!SUBSCRIPTION_PROVIDERS.has(provider)) return null;
  if (unavailable) {
    return (
      <section className="settings-provider-funding">
        <h3>{t("settings.funding.title")}</h3>
        <p className="muted">{t("settings.funding.unavailable")}</p>
      </section>
    );
  }
  if (!status) return null;

  return (
    <section
      className="settings-provider-funding"
      data-testid="provider-funding-status"
    >
      <div className="section-heading">
        <h3>{t("settings.funding.title")}</h3>
        <Badge tone="neutral">{status.scope}</Badge>
      </div>
      <p className="muted">{t("settings.funding.hint")}</p>
      <dl className="request-facts">
        <div>
          <dt>{t("request.funding.providerEvidence")}</dt>
          <dd>{t("request.funding.providerEvidenceMissing")}</dd>
        </div>
        <div>
          <dt>{t("request.funding.guard")}</dt>
          <dd>{t("request.funding.guardNotSatisfied")}</dd>
        </div>
        <div>
          <dt>{t("request.funding.extraUsage")}</dt>
          <dd>
            <code>{status.extraUsage}</code>
          </dd>
        </div>
        <div>
          <dt>{t("request.funding.missingEvidence")}</dt>
          <dd>
            <ul className="request-value-list">
              {status.missingEvidence.map((item) => (
                <li key={item}>
                  <code>{item}</code>
                </li>
              ))}
            </ul>
          </dd>
        </div>
      </dl>
      <p className="muted">{status.note}</p>
    </section>
  );
}
