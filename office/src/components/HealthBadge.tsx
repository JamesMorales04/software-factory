import { useEffect, useState } from "react";
import { coreApi } from "../coreApi";

type HealthState = "checking" | "healthy" | "unhealthy";

export function HealthBadge() {
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
      ? "Core healthy"
      : state === "unhealthy"
        ? "Core unreachable"
        : "Checking Core";

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
