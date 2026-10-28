import { useEffect, useState } from "react";
import { coreApi } from "../coreApi";
import { useI18n } from "../i18n";

type HealthState = "checking" | "healthy" | "unhealthy";

export function HealthBadge() {
  const { t } = useI18n();
  const [state, setState] = useState<HealthState>("checking");

  useEffect(() => {
    let active = true;
    async function check() {
      try {
        const health = await coreApi.health();
        if (active) setState(health.healthy ? "healthy" : "unhealthy");
      } catch {
        if (active) setState("unhealthy");
      }
    }
    void check();
    const timer = setInterval(() => void check(), 10000);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, []);

  const label =
    state === "healthy"
      ? t("shell.health.healthy")
      : state === "unhealthy"
        ? t("shell.health.unreachable")
        : t("shell.health.checking");

  return (
    <span
      className={`health-badge health-badge--${state}`}
      role="status"
      aria-live="polite"
      title={label}
    >
      <span className="health-dot" aria-hidden="true" />
      <span className="health-text">{label}</span>
    </span>
  );
}
