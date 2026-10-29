import type { ParticipantRecord, ParticipantRole } from "../../coreApi";
import { shortId } from "../../format";
import type { MessageKey, Translate } from "../../i18n";

const ROLE_LABEL_KEYS: Record<ParticipantRole, MessageKey> = {
  IMPLEMENTER: "agentOffice.role.IMPLEMENTER",
  LEAD: "agentOffice.role.LEAD",
  VERIFIER: "agentOffice.role.VERIFIER",
  REVIEWER: "agentOffice.role.REVIEWER",
  WORKER: "agentOffice.role.WORKER",
};

export function roleLabel(role: ParticipantRole, t: Translate): string {
  return t(ROLE_LABEL_KEYS[role]);
}

export function routeLabel(
  participant: ParticipantRecord,
  t: Translate,
): string {
  const route = participant.requested_route ?? participant.observed_route;
  if (!route) return t("agentOffice.route.notExposed");
  const source = participant.requested_route
    ? t("agentOffice.route.requested")
    : t("agentOffice.route.observed");
  return `${route.provider}/${route.model} (${source})`;
}

export function assignmentLabel(
  participant: ParticipantRecord,
  t: Translate,
): string {
  if (participant.work_unit_id && participant.campaign_id) {
    return t("agentOffice.assignment.unitCampaign", {
      unit: participant.work_unit_id,
      campaign: shortId(participant.campaign_id),
    });
  }
  if (participant.campaign_id) {
    return t("agentOffice.assignment.campaign", {
      id: shortId(participant.campaign_id),
    });
  }
  if (participant.request_id) {
    return t("agentOffice.assignment.leadRequest", {
      id: shortId(participant.request_id),
    });
  }
  return t("agentOffice.assignment.none");
}

export function observationLabel(participant: ParticipantRecord): string {
  if (participant.observation === "STALE") return "STALE";
  if (participant.observation === "CURRENT") return "CURRENT";
  if (participant.observation === "FINAL") return "FINAL";
  if (participant.observation === "NOT_EXPOSED") return "NOT EXPOSED";
  return "UNKNOWN";
}
