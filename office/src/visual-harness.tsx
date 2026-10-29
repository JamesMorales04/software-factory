import "@fontsource-variable/ibm-plex-sans";
import "@fontsource/ibm-plex-mono/400.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { AgentOfficeIsometric } from "./components/office/AgentOfficeIsometric";
import type { ParticipantRecord, ParticipantRole } from "./coreApi";
import { I18nProvider } from "./i18n";
import "./styles.css";
import "./visual-harness.css";

const ROLES: ParticipantRole[] = [
  "LEAD",
  "IMPLEMENTER",
  "VERIFIER",
  "REVIEWER",
  "WORKER",
];

function participant(index: number, role: ParticipantRole): ParticipantRecord {
  const id = `participant-${String(index).padStart(2, "0")}-session`;
  return {
    participant_id: `${role.toLowerCase()}:${id}`,
    role,
    identity_kind: "PROVIDER_SESSION",
    identity_id: id,
    principal_id: `agent-${index}`,
    project_id: "project-1",
    action_id: "action-1",
    campaign_id: "campaign-1",
    request_id: null,
    work_unit_id: `unit-${index}`,
    run_id: "run-1",
    attempt_id: `attempt-${index}`,
    attempt_generation: 1,
    worker_id: "worker-1",
    requested_route: { provider: "opencode", model: "deepseek", mode: "build" },
    observed_route: null,
    route_provenance: "PROVIDER_SESSION_BINDING_REQUESTED",
    session: null,
    status: allActive ? "ACTIVE" : index % 7 === 0 ? "COMPLETED" : "ACTIVE",
    status_source: "PROVIDER_SESSION",
    lease_state: allActive ? "LIVE" : index % 7 === 0 ? "RELEASED" : "LIVE",
    observation: allActive
      ? "CURRENT"
      : index % 7 === 0
        ? "FINAL"
        : index % 5 === 0
          ? "STALE"
          : "CURRENT",
    failure_reason: null,
    usage: null,
    usage_provenance: "PROVIDER_SESSION_BINDING",
    evidence: null,
    model_call_count: null,
    model_call_state: null,
    observation_note:
      "Renderer fixture only. Not a live Core projection and not recorded activity.",
    recorded_at: "2026-10-28T10:00:00Z",
    last_observed_at: "2026-10-28T10:05:00Z",
  };
}

const params = new URLSearchParams(window.location.search);
const count = Number(params.get("count") ?? "5");
const mixed = params.get("mode") !== "implementers";
const allActive = params.get("allActive") === "1";
const locale = params.get("locale") === "es" ? "es" : "en";
const participants = Array.from({ length: count }, (_, index) =>
  participant(index, mixed ? ROLES[index % ROLES.length] : "IMPLEMENTER"),
);
const selected = params.get("selected")
  ? (participants.find((item) => item.identity_id === params.get("selected"))
      ?.participant_id ?? null)
  : null;

const root = document.getElementById("root");
if (!root) throw new Error("missing #root");
createRoot(root).render(
  <StrictMode>
    <I18nProvider initialLocale={locale}>
      <div className="harness">
        <header className="harness-header">
          <p className="harness-kicker">TEST FIXTURE — NOT LIVE CORE DATA</p>
          <h1>Agent Office renderer preview</h1>
          <p>
            {count} deterministic fixture participants ·{" "}
            {mixed ? "mixed roles" : "Implementers only"} · renderer review
            only; no telemetry, activity or acceptance is implied.
          </p>
        </header>
        <AgentOfficeIsometric
          participants={participants}
          selectedId={selected}
          onSelect={() => undefined}
        />
      </div>
    </I18nProvider>
  </StrictMode>,
);
