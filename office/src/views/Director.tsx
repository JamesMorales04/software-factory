import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { Link, useSearchParams } from "react-router-dom";
import { Badge } from "../components/ui/Badge";
import { Button } from "../components/ui/Button";
import { Input, Select, Textarea } from "../components/ui/Field";
import {
  EmptyState,
  ErrorState,
  LoadingState,
  PageHeader,
  Panel,
} from "../components/ui/Surface";
import {
  type AgenticAssurance,
  type AgenticParticipantPlan,
  type AgenticPlanningProjection,
  type AgenticPortfolioEntry,
  type AgenticRequestDetail,
  type AgenticRoute,
  type AgenticSpecDetail,
  type AgenticWorkGraph,
  type AgenticWorkGraphRecord,
  CoreApiError,
  coreApi,
  recordedPlanReview,
} from "../coreApi";
import { shortId } from "../format";
import { useI18n } from "../i18n";
import { EnvironmentPanel, environmentStatusLabel } from "./EnvironmentPanel";
import { KnowledgePanel } from "./KnowledgePanel";
import { currentPlanningReview, isTerminalIntakeState } from "./RequestState";
import "./director.css";
import "./specs.css";

function errorText(cause: unknown, fallback: string): string {
  return cause instanceof CoreApiError ? cause.message : fallback;
}

/// Core reports an absent validated WorkGraph as a typed state, not as a
/// fabricated one-unit plan. The UI keeps an empty, clearly non-executable
/// participant shape so existing layout code stays total, while every pending
/// surface is driven by `participantState`/`workGraphState`.
const EMPTY_PARTICIPANT_PLAN: AgenticParticipantPlan = {
  route: "NO_AGENT",
  assurance: "NONE",
  assignments: [],
  contextPolicy: {
    compactAtPercent: 70,
    maxConsecutiveCompactions: 2,
    handoffAtStableMilestone: true,
    requireProbeAfterHandoff: true,
  },
  reasons: [],
  unitAssignments: [],
  reviewerCount: 0,
  reviewerRoles: [],
  reviewerAssignments: [],
};

export type NormalizedRequestDetail = AgenticRequestDetail & {
  participants: AgenticParticipantPlan;
};

export function withParticipantFallback(
  detail: AgenticRequestDetail,
): NormalizedRequestDetail {
  return detail.participants
    ? (detail as NormalizedRequestDetail)
    : { ...detail, participants: EMPTY_PARTICIPANT_PLAN };
}

function routeTone(route: AgenticRoute) {
  switch (route) {
    case "NO_AGENT":
      return "neutral" as const;
    case "DIRECT":
      return "info" as const;
    case "DELEGATED":
      return "attention" as const;
    case "FORMAL_SPEC":
      return "success" as const;
    case "REPAIR":
      return "danger" as const;
  }
}

function assuranceTone(assurance: AgenticAssurance) {
  switch (assurance) {
    case "NONE":
      return "neutral" as const;
    case "STANDARD":
      return "info" as const;
    case "ELEVATED":
      return "attention" as const;
    case "CRITICAL":
      return "danger" as const;
  }
}

function reviewerReceiptTone(result: string) {
  if (result === "PASS") return "success" as const;
  if (result === "FAIL" || result === "CHANGES_REQUESTED") {
    return "danger" as const;
  }
  return "attention" as const;
}

type PreparationPhase =
  | "PREPARING"
  | "WAITING_OWNER"
  | "REVIEW_REJECTED"
  | "RECOVERABLE_FAILURE"
  | "EXECUTING"
  | "TERMINAL"
  | "READY";

export function reviewerAssignmentsMatch(
  detail: AgenticRequestDetail,
): boolean {
  const roles = detail.participants?.reviewerRoles ?? [];
  const assignments = detail.participants?.reviewerAssignments ?? [];
  if (roles.length === 0) return assignments.length === 0;
  return (
    assignments.length === roles.length &&
    roles.every((role) => assignments.some((entry) => entry.role === role))
  );
}

export function Director() {
  const { t, formatDate } = useI18n();
  const [searchParams, setSearchParams] = useSearchParams();
  const selectedId = searchParams.get("request") ?? "";
  const [portfolio, setPortfolio] = useState<AgenticPortfolioEntry[] | null>(
    null,
  );
  const [portfolioError, setPortfolioError] = useState<string | null>(null);
  const [detail, setDetail] = useState<NormalizedRequestDetail | null>(null);
  const [detailError, setDetailError] = useState<string | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [refreshBusy, setRefreshBusy] = useState(false);
  const [startPollExhausted, setStartPollExhausted] = useState(false);
  const [preparationStalled, setPreparationStalled] = useState(false);
  const [triageBusy, setTriageBusy] = useState(false);
  const [triageError, setTriageError] = useState<string | null>(null);
  const [triageNotice, setTriageNotice] = useState<string | null>(null);
  const [assessmentBusy, setAssessmentBusy] = useState(false);
  const [assessmentError, setAssessmentError] = useState<string | null>(null);
  const [assessmentNotice, setAssessmentNotice] = useState<string | null>(null);
  const [lastLoadedAt, setLastLoadedAt] = useState<string | null>(null);
  const [scheduler, setScheduler] = useState<{
    capacity: number;
    active: number;
    queued: number;
    resourcePolicy: string;
    worktreePolicy: string;
  } | null>(null);
  const [priorityDraft, setPriorityDraft] = useState("100");
  const [priorityBusy, setPriorityBusy] = useState(false);
  const [priorityError, setPriorityError] = useState<string | null>(null);
  const [priorityNotice, setPriorityNotice] = useState<string | null>(null);
  const [plan, setPlan] = useState<AgenticWorkGraphRecord | null>(null);
  const [planBusy, setPlanBusy] = useState(false);
  const [planError, setPlanError] = useState<string | null>(null);
  const [planNotice, setPlanNotice] = useState<string | null>(null);
  const [participantBusy, setParticipantBusy] = useState(false);
  const [spec, setSpec] = useState<AgenticSpecDetail | null>(null);
  const [specBusy, setSpecBusy] = useState<string | null>(null);
  const [specError, setSpecError] = useState<string | null>(null);
  const [specNotice, setSpecNotice] = useState<string | null>(null);
  const [planning, setPlanning] = useState<AgenticPlanningProjection | null>(
    null,
  );
  const [planningError, setPlanningError] = useState<string | null>(null);
  const [planningNotice, setPlanningNotice] = useState<string | null>(null);
  const [reviewBusy, setReviewBusy] = useState<{
    requestId: string;
    decision: string;
  } | null>(null);
  const [reviewRationale, setReviewRationale] = useState("");
  // Core records the owner decision before it prepares the environment. When a
  // later phase fails, the decision is durable even though the projection may
  // still show the previous review. This local acknowledgement keeps the
  // recorded fact visible and stops inviting a duplicate decision until a
  // reload confirms it.
  const [recordedDecision, setRecordedDecision] = useState<{
    requestId: string;
  } | null>(null);
  const [findingId, setFindingId] = useState("");
  const [findingEvidence, setFindingEvidence] = useState("");
  const [findingOwnerDecision, setFindingOwnerDecision] = useState("");
  const [findingBusy, setFindingBusy] = useState<string | null>(null);
  const [findingError, setFindingError] = useState<string | null>(null);
  const [findingNotice, setFindingNotice] = useState<string | null>(null);

  const loadPortfolio = useCallback(async () => {
    try {
      const next = await coreApi.listAgenticPortfolio();
      setPortfolio(next);
      setPortfolioError(null);
      setLastLoadedAt(new Date().toISOString());
    } catch (cause) {
      setPortfolioError(errorText(cause, t("director.error.portfolio")));
    }
  }, [t]);

  useEffect(() => {
    void loadPortfolio();
    const timer = window.setInterval(() => void loadPortfolio(), 8000);
    return () => window.clearInterval(timer);
  }, [loadPortfolio]);
  useEffect(() => {
    const load = async () => {
      try {
        setScheduler(await coreApi.getAgenticScheduler());
      } catch {
        setScheduler(null);
      }
    };
    void load();
    const timer = window.setInterval(() => void load(), 5000);
    return () => window.clearInterval(timer);
  }, []);

  const graph = useMemo<AgenticWorkGraph | null>(() => {
    if (!plan) return null;
    try {
      return JSON.parse(plan.graphJson) as AgenticWorkGraph;
    } catch {
      return null;
    }
  }, [plan]);

  const activeId =
    selectedId ||
    (portfolio !== null && portfolio.length > 0 ? portfolio[0].id : "");
  // A plan-review response is only allowed to update the request that issued
  // it. A single in-flight guard also prevents a double click from posting the
  // same decision twice before React re-renders the disabled button.
  const activeRequestRef = useRef(activeId);
  // The in-flight guard is scoped per request, so a pending decision for one
  // request never blocks a valid review of another.
  const reviewInFlight = useRef<Set<string>>(new Set());

  const environment = detail?.environment ?? null;
  const reviewBinding = currentPlanningReview(planning);
  const reviewIsCurrent = reviewBinding.current;
  const reviewStatus = reviewBinding.status;
  const activeReviewBusy =
    reviewBusy?.requestId === activeId ? reviewBusy.decision : null;
  const reviewRejected =
    reviewStatus === "REJECTED" || reviewStatus === "CHANGES_REQUESTED";
  const requestTerminal = isTerminalIntakeState(detail?.request.state ?? "");
  const executionActive =
    detail?.request.state === "EXECUTING" || detail?.lead.campaignId !== null;
  // A current PENDING review is the first decision. A current REJECTED or
  // CHANGES_REQUESTED review stays revisitable: Core accepts a new
  // authenticated decision on the same plan revision and records it as the new
  // effective decision. A stale, terminal or executing request never exposes
  // the form, and no decision is inferred or auto-submitted.
  const reviewRevisitable =
    reviewStatus === "REJECTED" || reviewStatus === "CHANGES_REQUESTED";
  const reviewDecisionAvailable =
    planning !== null &&
    planning.review !== null &&
    reviewIsCurrent &&
    !requestTerminal &&
    !executionActive &&
    (reviewStatus === "PENDING" || reviewRevisitable);
  const operationInconsistent =
    detail?.operation?.status === "EXECUTING" &&
    detail?.lead.campaignId === null &&
    detail?.request.state !== "EXECUTING";
  const terminalFailure =
    detail?.request.state === "BLOCKED" ||
    detail?.request.state === "FAILED" ||
    detail?.lead.state === "BLOCKED" ||
    detail?.lead.state === "FAILED";
  const environmentBlocked =
    environment?.required === true &&
    (environment.status === "BLOCKED" || environment.blockers.length > 0);
  const startReady = planning?.startReady === true;
  // A PENDING review is only a real owner wait when Core binds it to the
  // current PLAN revision and reports the materials as ready. A stale review
  // or still-missing materials stay in preparation.
  const waitingForOwner =
    reviewStatus === "PENDING" &&
    planning?.reviewCurrent === true &&
    planning?.materialsReady === true &&
    planning?.ready !== true;
  const preparationPhase: PreparationPhase = requestTerminal
    ? "TERMINAL"
    : executionActive
      ? "EXECUTING"
      : reviewRejected
        ? "REVIEW_REJECTED"
        : terminalFailure || environmentBlocked || operationInconsistent
          ? "RECOVERABLE_FAILURE"
          : startReady
            ? "READY"
            : waitingForOwner
              ? "WAITING_OWNER"
              : "PREPARING";
  const preparationPhaseMessage =
    preparationPhase === "TERMINAL"
      ? t("director.preparation.phase.terminal")
      : preparationPhase === "EXECUTING"
        ? t("director.preparation.phase.executing")
        : preparationPhase === "REVIEW_REJECTED"
          ? t("director.preparation.phase.reviewRejected")
          : preparationPhase === "RECOVERABLE_FAILURE"
            ? t("director.preparation.phase.recoverableFailure")
            : preparationPhase === "READY"
              ? t("director.preparation.planningReady")
              : preparationPhase === "WAITING_OWNER"
                ? t("director.preparation.phase.waitingOwner")
                : t("director.preparation.phase.preparing");

  useEffect(() => {
    // Action-scoped notices belong to the request that produced them. Keeping
    // them across a selection change showed a success message from one request
    // on another request's panel.
    if (activeId === "") return;
    setPriorityNotice(null);
    setPriorityError(null);
    setPlanNotice(null);
    setPlanError(null);
    setSpecNotice(null);
    setSpecError(null);
    setPlanningNotice(null);
    setPlanningError(null);
    setReviewRationale("");
    setRecordedDecision(null);
    setFindingId("");
    setFindingEvidence("");
    setFindingOwnerDecision("");
    setFindingNotice(null);
    setFindingError(null);
    setTriageNotice(null);
    setTriageError(null);
    setAssessmentNotice(null);
    setAssessmentError(null);
  }, [activeId]);

  useLayoutEffect(() => {
    // The in-flight response guard must observe the selected request before any
    // user event or asynchronous response runs, so a late handler cannot clear
    // or write state for the request the operator is viewing now.
    activeRequestRef.current = activeId;
  }, [activeId]);

  const loadGeneration = useRef(0);
  const lastResumeReload = useRef(0);
  const startPollCount = useRef(0);
  const preparationFailureCount = useRef(0);

  /**
   * Core owns every readiness fact. This reload keeps Director's detail, plan,
   * spec and planning projections on the same generation so a mutation is
   * never displayed next to stale readiness. A failed fetch preserves the last
   * confirmed projection and reports the error separately; it is not treated
   * as a new authoritative state.
   */
  const reloadDirector = useCallback(
    async ({ reset = false }: { reset?: boolean } = {}) => {
      if (!activeId) {
        loadGeneration.current += 1;
        setDetail(null);
        setDetailError(null);
        setDetailLoading(false);
        setPlan(null);
        setPlanError(null);
        setSpec(null);
        setSpecError(null);
        setPlanning(null);
        setPlanningError(null);
        return;
      }
      const generation = ++loadGeneration.current;
      if (reset) {
        setDetail(null);
        setDetailLoading(true);
        setDetailError(null);
        setPlan(null);
        setPlanError(null);
        setSpec(null);
        setSpecError(null);
        setPlanning(null);
        setPlanningError(null);
      }
      const [detailResult, planResult, specResult, planningResult] =
        await Promise.allSettled([
          coreApi.getAgenticRequest(activeId),
          coreApi.getAgenticPlan(activeId),
          coreApi.getAgenticSpec(activeId),
          coreApi.getAgenticPlanning(activeId),
        ]);
      if (generation !== loadGeneration.current) return;
      const criticalFailure =
        detailResult.status === "rejected" ||
        planningResult.status === "rejected";
      if (criticalFailure) {
        preparationFailureCount.current += 1;
        if (preparationFailureCount.current >= 6) {
          setPreparationStalled(true);
        }
      } else {
        preparationFailureCount.current = 0;
        setPreparationStalled(false);
      }
      if (detailResult.status === "fulfilled") {
        setDetail(withParticipantFallback(detailResult.value));
        setDetailError(null);
        setLastLoadedAt(new Date().toISOString());
        if (detailResult.value.operation) {
          setPriorityDraft(String(detailResult.value.operation.priority));
        }
      } else {
        setDetailError(
          errorText(detailResult.reason, t("director.error.detail")),
        );
      }
      if (planResult.status === "fulfilled") {
        setPlan(planResult.value.graph);
        setPlanError(null);
      } else {
        setPlanError(errorText(planResult.reason, t("director.plan.error")));
      }
      if (specResult.status === "fulfilled") {
        setSpec(specResult.value);
        setSpecError(null);
      } else {
        setSpecError(errorText(specResult.reason, t("director.spec.error")));
      }
      if (planningResult.status === "fulfilled") {
        setPlanning(planningResult.value);
        setPlanningError(null);
      } else {
        setPlanningError(
          errorText(planningResult.reason, t("director.planning.error")),
        );
      }
      setDetailLoading(false);
    },
    [activeId, t],
  );

  useEffect(() => {
    void reloadDirector({ reset: true });
    return () => {
      loadGeneration.current += 1;
    };
  }, [reloadDirector]);

  useEffect(() => {
    // Reset bounded polling state when the selected request changes.
    void activeId;
    startPollCount.current = 0;
    setStartPollExhausted(false);
    preparationFailureCount.current = 0;
    setPreparationStalled(false);
  }, [activeId]);

  useEffect(() => {
    // Reset the bounded start window when any authoritative start input moves.
    void activeId;
    void startReady;
    void detail?.lead.campaignId;
    void detail?.request.state;
    void detail?.operation?.status;
    startPollCount.current = 0;
    setStartPollExhausted(false);
  }, [
    activeId,
    startReady,
    detail?.lead.campaignId,
    detail?.request.state,
    detail?.operation?.status,
  ]);

  const queuedOrStarting =
    detail?.operation?.status === "QUEUED" ||
    detail?.operation?.status === "STARTING";
  const startsPending =
    startReady &&
    queuedOrStarting &&
    !detail?.lead.campaignId &&
    !terminalFailure &&
    !environmentBlocked &&
    !startPollExhausted;
  const preparationPending =
    detail?.request.mode === "AUTONOMOUS" &&
    !detail?.lead.campaignId &&
    !environmentBlocked &&
    !preparationStalled &&
    queuedOrStarting &&
    (preparationPhase === "PREPARING" || startsPending);

  useEffect(() => {
    if (!preparationPending) return;
    const timer = window.setInterval(() => {
      if (typeof document !== "undefined" && document.hidden) return;
      if (startsPending) {
        startPollCount.current += 1;
        if (startPollCount.current >= 6) {
          setStartPollExhausted(true);
          return;
        }
      }
      void reloadDirector();
    }, 5000);
    return () => window.clearInterval(timer);
  }, [preparationPending, startsPending, reloadDirector]);

  // A stopped wait must still observe an approval recorded by another tab or
  // device. Focus/visibility are existing browser signals, not a new
  // subscription: one shared reload with a short de-duplication window avoids
  // duplicated requests when both events fire together.
  useEffect(() => {
    if (preparationPhase !== "WAITING_OWNER") return;
    const resume = () => {
      const now = Date.now();
      if (now - lastResumeReload.current < 1000) return;
      lastResumeReload.current = now;
      void reloadDirector();
    };
    const onVisibility = () => {
      if (document.visibilityState === "visible") resume();
    };
    window.addEventListener("focus", resume);
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      window.removeEventListener("focus", resume);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [preparationPhase, reloadDirector]);

  useEffect(() => {
    if (recordedDecision === null) return;
    if (recordedDecision.requestId !== activeId) {
      setRecordedDecision(null);
      return;
    }
    // A reload that no longer reports a current PENDING review has caught up
    // with the recorded decision; the projection is now the authority again.
    if (planning !== null && (!reviewIsCurrent || reviewStatus !== "PENDING")) {
      setRecordedDecision(null);
    }
  }, [activeId, planning, recordedDecision, reviewIsCurrent, reviewStatus]);

  async function runSemanticTriage() {
    if (!activeId) return;
    setTriageBusy(true);
    setTriageError(null);
    setTriageNotice(null);
    try {
      await coreApi.assessAgenticRequest(activeId);
      await reloadDirector();
      setTriageNotice(t("director.triage.success"));
    } catch (cause) {
      setTriageError(errorText(cause, t("director.triage.error")));
    } finally {
      setTriageBusy(false);
    }
  }

  async function runLeadAssessment() {
    if (!activeId) return;
    setAssessmentBusy(true);
    setAssessmentError(null);
    setAssessmentNotice(null);
    try {
      await coreApi.assessIntakeRequest(activeId);
      await reloadDirector();
      setAssessmentNotice(t("director.plan.assessmentSuccess"));
    } catch (cause) {
      setAssessmentError(errorText(cause, t("director.plan.assessmentError")));
    } finally {
      setAssessmentBusy(false);
    }
  }

  async function updatePriority() {
    if (!activeId) return;
    const priority = Number(priorityDraft);
    if (!Number.isInteger(priority) || priority < 0 || priority > 1000) {
      setPriorityError(t("director.priority.invalid"));
      return;
    }
    setPriorityBusy(true);
    setPriorityError(null);
    setPriorityNotice(null);
    try {
      await coreApi.setAgenticPriority(activeId, priority);
      await reloadDirector();
      setPriorityNotice(t("director.priority.success"));
    } catch (cause) {
      setPriorityError(errorText(cause, t("director.priority.error")));
    } finally {
      setPriorityBusy(false);
    }
  }

  async function compilePlan() {
    if (!activeId) return;
    setPlanBusy(true);
    setPlanError(null);
    setPlanNotice(null);
    try {
      const result = await coreApi.planAgenticRequest(activeId);
      setPlan(result.graph);
      await reloadDirector();
      setPlanNotice(t("director.plan.success"));
    } catch (cause) {
      setPlanError(errorText(cause, t("director.plan.error")));
    } finally {
      setPlanBusy(false);
    }
  }

  async function recordParticipants() {
    if (!activeId) return;
    setParticipantBusy(true);
    setPlanError(null);
    setPlanNotice(null);
    try {
      await coreApi.recordAgenticParticipants(activeId);
      await reloadDirector();
      setPlanNotice(t("director.participants.success"));
    } catch (cause) {
      setPlanError(errorText(cause, t("director.participants.error")));
    } finally {
      setParticipantBusy(false);
    }
  }

  async function authorSpec() {
    if (!activeId) return;
    setSpecBusy("author");
    setSpecError(null);
    setSpecNotice(null);
    try {
      const result = await coreApi.authorAgenticSpec(activeId);
      await reloadDirector();
      setSpecNotice(
        t("director.spec.success", { revision: result.spec.revision }),
      );
    } catch (cause) {
      setSpecError(errorText(cause, t("director.spec.error")));
    } finally {
      setSpecBusy(null);
    }
  }

  async function freezeSpec() {
    if (!activeId || !spec?.draft?.preview) return;
    setSpecBusy("freeze");
    setSpecError(null);
    setSpecNotice(null);
    try {
      const result = await coreApi.freezeAgenticSpec(
        activeId,
        spec.draft.preview.specSha256,
      );
      await reloadDirector();
      setSpecNotice(
        t("director.spec.freezeSuccess", { revision: result.spec.revision }),
      );
    } catch (cause) {
      setSpecError(errorText(cause, t("director.spec.freezeError")));
    } finally {
      setSpecBusy(null);
    }
  }

  async function reviewPlan(
    decision: "APPROVE" | "CHANGES_REQUESTED" | "REJECT",
  ) {
    if (!activeId || !planning?.review) return;
    const requestId = activeId;
    if (reviewInFlight.current.has(requestId)) return;
    if (reviewRationale.trim() === "") {
      setPlanningError(t("director.planning.rationaleRequired"));
      return;
    }
    const planRevision = planning.review.planRevision;
    const planSha256 = planning.review.planSha256;
    const stillCurrent = () => activeRequestRef.current === requestId;
    reviewInFlight.current.add(requestId);
    setReviewBusy({ requestId, decision });
    setPlanningError(null);
    setPlanningNotice(null);
    try {
      const outcome = await coreApi.reviewAgenticPlan(requestId, {
        planRevision,
        planSha256,
        decision,
        rationale: reviewRationale.trim(),
      });
      if (!stillCurrent()) return;
      // Core records the decision before it prepares the environment. Office
      // may only acknowledge it when the response confirms the recorded
      // decision for the exact revision it submitted; a 2xx alone is not proof.
      const confirmed =
        outcome.reviewRecorded === true &&
        outcome.review?.planRevision === planRevision;
      if (!confirmed) {
        await reloadDirector();
        if (!stillCurrent()) return;
        setPlanningNotice(t("director.planning.reviewNotConfirmed"));
        return;
      }
      const environmentBlocked = outcome.environmentBlocked === true;
      setRecordedDecision({ requestId });
      await reloadDirector();
      if (!stillCurrent()) return;
      setReviewRationale("");
      setPlanningNotice(
        environmentBlocked
          ? t("director.planning.reviewRecordedEnvironmentBlocked")
          : t("director.planning.reviewSuccess"),
      );
    } catch (cause) {
      const recorded = recordedPlanReview(cause);
      const confirmed =
        recorded !== null && recorded.reviewRevision === planRevision;
      if (!confirmed) {
        if (!stillCurrent()) return;
        const code = cause instanceof CoreApiError ? cause.code : null;
        if (
          code === "PLAN_REVIEW_STALE" ||
          code === "PLAN_NOT_READY" ||
          code === "INTAKE_WITHDRAWN"
        ) {
          // The projection is the authority for the current revision, the
          // missing materials or the withdrawn state. Never retry the decision
          // with the values that Core just rejected.
          await reloadDirector();
          if (!stillCurrent()) return;
        }
        setPlanningError(errorText(cause, t("director.planning.reviewError")));
        return;
      }
      // Core persisted the owner decision and reported the later blocker in the
      // same response. Re-read the projection and acknowledge the durable fact
      // instead of asking the owner to approve an already recorded decision.
      // The acknowledgement and rationale clear happen only after the guard:
      // a stale response from another request must not touch this one.
      await reloadDirector();
      if (!stillCurrent()) return;
      setRecordedDecision({ requestId });
      setReviewRationale("");
      setPlanningNotice(
        t("director.planning.reviewRecordedPreparationPending", {
          detail: recorded.message,
        }),
      );
    } finally {
      reviewInFlight.current.delete(requestId);
      setReviewBusy((current) =>
        current?.requestId === requestId ? null : current,
      );
    }
  }

  async function disposeFinding(status: "RESOLVED" | "REFUTED") {
    if (!activeId || !findingId) return;
    const evidenceRefs = findingEvidence
      .split(/[\n,]/)
      .map((value) => value.trim())
      .filter(Boolean);
    if (evidenceRefs.length === 0) {
      setFindingError(t("director.findings.evidenceRequired"));
      return;
    }
    if (status === "REFUTED" && findingOwnerDecision.trim() === "") {
      setFindingError(t("director.findings.ownerDecisionRequired"));
      return;
    }
    setFindingBusy(status);
    setFindingError(null);
    setFindingNotice(null);
    try {
      await coreApi.resolveAgenticFinding(activeId, findingId, {
        status,
        evidenceRefs,
        ownerDecision:
          status === "REFUTED" ? findingOwnerDecision.trim() : undefined,
      });
      await reloadDirector();
      setFindingId("");
      setFindingEvidence("");
      setFindingOwnerDecision("");
      setFindingNotice(t("director.findings.success"));
    } catch (cause) {
      setFindingError(errorText(cause, t("director.findings.error")));
    } finally {
      setFindingBusy(null);
    }
  }

  function artifact<T>(kind: string): T | null {
    const record = planning?.artifacts.find(
      (candidate) => candidate.kind === kind,
    );
    if (!record) return null;
    try {
      return JSON.parse(record.contentJson) as T;
    } catch {
      return null;
    }
  }

  function selectRequest(id: string) {
    setSearchParams(
      (current) => {
        const next = new URLSearchParams(current);
        next.set("request", id);
        return next;
      },
      { replace: true },
    );
  }

  const selectedEntry = portfolio?.find((entry) => entry.id === activeId);
  const participantPlan = planning?.participantPlan ?? null;
  const artifactsPrepared =
    planning !== null && planning.missingArtifacts.length === 0;
  const participantsPrepared =
    participantPlan !== null &&
    (!participantPlan.required || participantPlan.ready);

  async function refreshDirector() {
    setRefreshBusy(true);
    startPollCount.current = 0;
    setStartPollExhausted(false);
    try {
      await Promise.all([loadPortfolio(), reloadDirector()]);
    } finally {
      setRefreshBusy(false);
    }
  }

  return (
    <div className="director-workspace">
      <PageHeader
        title={t("director.title")}
        description={t("director.description")}
        meta={
          lastLoadedAt && (
            <span>
              {t("director.updated", { date: formatDate(lastLoadedAt) })}
            </span>
          )
        }
        actions={
          <Button
            variant="secondary"
            size="sm"
            busy={refreshBusy}
            disabled={refreshBusy}
            onClick={() => void refreshDirector()}
          >
            {t("director.refresh")}
          </Button>
        }
      />

      {portfolioError && (
        <ErrorState
          message={portfolioError}
          action={
            <Button size="sm" onClick={() => void loadPortfolio()}>
              {t("common.retry")}
            </Button>
          }
        />
      )}

      {portfolio === null && portfolioError === null && (
        <LoadingState label={t("director.loading")} />
      )}

      {portfolio !== null && portfolio.length === 0 && (
        <EmptyState
          title={t("director.empty.title")}
          description={t("director.empty.description")}
        />
      )}

      {portfolio !== null && portfolio.length > 0 && (
        <div className="director-layout">
          <Panel className="director-portfolio">
            <div className="section-heading">
              <h2>{t("director.portfolio.title")}</h2>
              <span className="muted">
                {t("director.portfolio.count", { count: portfolio.length })}
              </span>
            </div>
            <ul
              className="director-list"
              aria-label={t("director.portfolio.title")}
            >
              {portfolio.map((entry) => (
                <li key={entry.id}>
                  <button
                    type="button"
                    className={`director-list-item ${
                      entry.id === activeId ? "director-list-item--active" : ""
                    }`}
                    aria-current={entry.id === activeId ? "true" : undefined}
                    onClick={() => selectRequest(entry.id)}
                  >
                    <span className="director-list-title">
                      {entry.objective}
                    </span>
                    <span className="director-list-meta">
                      {shortId(entry.projectId)}
                      {" · "}
                      {entry.state}
                    </span>
                    <span className="director-list-badges">
                      <Badge tone={routeTone(entry.route)}>{entry.route}</Badge>
                      <Badge tone={assuranceTone(entry.assurance)}>
                        {entry.assurance}
                      </Badge>
                      <span className="muted">
                        {t("director.portfolio.agents", {
                          count: entry.participantCount,
                        })}
                      </span>
                      <Badge tone="neutral">
                        {t("director.portfolio.priority", {
                          priority: entry.priority,
                        })}
                      </Badge>
                      <span className="muted">{entry.operationStatus}</span>
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          </Panel>

          <div className="director-detail">
            {detailLoading && !detail && (
              <LoadingState label={t("director.detail.loading")} />
            )}
            {detailError && !detail && (
              <ErrorState
                message={detailError}
                action={
                  <Button
                    size="sm"
                    onClick={() => void reloadDirector({ reset: true })}
                  >
                    {t("common.retry")}
                  </Button>
                }
              />
            )}
            {detailError && detail && (
              <p className="ui-callout ui-callout--danger" role="alert">
                {detailError}{" "}
                <Button size="sm" onClick={() => void reloadDirector()}>
                  {t("common.retry")}
                </Button>
              </p>
            )}
            {!detailLoading && detail && (
              <>
                <Panel className="director-hero">
                  {triageNotice && (
                    <p className="notice" role="status" aria-live="polite">
                      {triageNotice}
                    </p>
                  )}
                  {triageError && (
                    <p className="ui-callout ui-callout--danger" role="alert">
                      {triageError}
                    </p>
                  )}
                  <div className="section-heading">
                    <div>
                      <p className="director-kicker">
                        {t("director.detail.request", {
                          id: shortId(detail.request.id),
                        })}
                      </p>
                      <h2>{detail.request.objective}</h2>
                    </div>
                    <div className="director-hero-actions">
                      <Badge tone={routeTone(detail.triage.route)}>
                        {detail.triage.route}
                      </Badge>
                      <Button
                        variant="secondary"
                        size="sm"
                        busy={triageBusy}
                        disabled={triageBusy}
                        onClick={() => void runSemanticTriage()}
                      >
                        {triageBusy
                          ? t("director.triage.running")
                          : t("director.triage.run")}
                      </Button>
                    </div>
                  </div>
                  <div className="director-badges">
                    <Badge tone="info">{detail.triage.requestKind}</Badge>
                    <Badge tone={assuranceTone(detail.triage.assurance)}>
                      {t("director.detail.assurance", {
                        value: detail.triage.assurance,
                      })}
                    </Badge>
                    <Badge tone="neutral">{detail.request.state}</Badge>
                    <Badge tone="neutral">
                      {t("director.detail.profile", {
                        name: detail.request.profile.name,
                        revision: detail.request.profile.revision,
                      })}
                    </Badge>
                  </div>
                  <dl className="director-facts">
                    <div>
                      <dt>{t("director.detail.project")}</dt>
                      <dd>{detail.request.projectId}</dd>
                    </div>
                    <div>
                      <dt>{t("director.detail.mode")}</dt>
                      <dd>{detail.request.mode}</dd>
                    </div>
                    <div>
                      <dt>{t("director.detail.mechanism")}</dt>
                      <dd>{detail.triage.mechanism}</dd>
                    </div>
                    <div>
                      <dt>{t("director.detail.plan")}</dt>
                      <dd>
                        {detail.plan ? (
                          <code title={detail.plan.planHash}>
                            {shortId(detail.plan.planHash, 14)}
                          </code>
                        ) : (
                          t("director.detail.noPlan")
                        )}
                      </dd>
                    </div>
                  </dl>
                  {detail.triage.reasons.length > 0 && (
                    <div className="director-reasons">
                      <h3>{t("director.detail.reasons")}</h3>
                      <ul>
                        {detail.triage.reasons.map((reason) => (
                          <li key={reason}>{reason}</li>
                        ))}
                      </ul>
                    </div>
                  )}
                  {detail.triage.unknowns.length > 0 && (
                    <p className="ui-callout ui-callout--attention">
                      {t("director.detail.unknowns", {
                        values: detail.triage.unknowns.join("; "),
                      })}
                    </p>
                  )}
                </Panel>

                <Panel>
                  <div className="section-heading">
                    <h2>{t("director.operation.title")}</h2>
                    <Badge tone="info">
                      {detail.operation?.status ??
                        t("director.operation.notRecorded")}
                    </Badge>
                  </div>
                  <dl className="director-facts">
                    <div>
                      <dt>{t("director.operation.director")}</dt>
                      <dd>{detail.operation?.directorId ?? "—"}</dd>
                    </div>
                    <div>
                      <dt>{t("director.operation.manager")}</dt>
                      <dd>{detail.operation?.operationManagerId ?? "—"}</dd>
                    </div>
                    <div>
                      <dt>{t("director.operation.priority")}</dt>
                      <dd>{detail.operation?.priority ?? 100}</dd>
                    </div>
                    <div>
                      <dt>{t("director.operation.worktree")}</dt>
                      <dd>
                        {detail.operation?.worktreePolicy ?? "PER_ATTEMPT"}
                      </dd>
                    </div>
                  </dl>
                  <div className="director-priority">
                    <Input
                      label={t("director.priority.label")}
                      type="number"
                      min={0}
                      max={1000}
                      value={priorityDraft}
                      onChange={(event) => setPriorityDraft(event.target.value)}
                      disabled={priorityBusy}
                    />
                    <Button
                      variant="secondary"
                      size="sm"
                      busy={priorityBusy}
                      disabled={priorityBusy}
                      onClick={() => void updatePriority()}
                    >
                      {t("director.priority.save")}
                    </Button>
                  </div>
                  <p className="muted">{t("director.priority.help")}</p>
                  {priorityNotice && (
                    <p className="notice" role="status">
                      {priorityNotice}
                    </p>
                  )}
                  {priorityError && (
                    <p className="ui-callout ui-callout--danger" role="alert">
                      {priorityError}
                    </p>
                  )}
                  {scheduler && (
                    <p className="ui-callout ui-callout--info" role="status">
                      {t("director.scheduler.value", {
                        capacity: scheduler.capacity,
                        active: scheduler.active,
                        queued: scheduler.queued,
                      })}
                    </p>
                  )}
                </Panel>

                <Panel>
                  <div className="section-heading">
                    <h2>{t("director.plan.title")}</h2>
                    {plan && (
                      <Badge tone="success">
                        {t("director.plan.units", {
                          count: graph?.units.length ?? 0,
                        })}
                      </Badge>
                    )}
                  </div>
                  {detail.participantState === "WORK_GRAPH_NOT_PLANNED" && (
                    <div className="ui-callout ui-callout--attention" role="status">
                      <p>
                        <strong>
                          {t("director.participants.workGraphNotPlannedTitle")}
                        </strong>{" "}
                        {t("director.participants.workGraphNotPlanned")}
                      </p>
                      <p className="muted">
                        <code>WORK_GRAPH_NOT_PLANNED</code>
                      </p>
                    </div>
                  )}
                  {planError && (
                    <p className="ui-callout ui-callout--danger" role="alert">
                      {planError}
                    </p>
                  )}
                  {planNotice && (
                    <p className="notice" role="status">
                      {planNotice}
                    </p>
                  )}
                  {plan && graph ? (
                    <>
                      <dl className="director-facts">
                        <div>
                          <dt>{t("director.plan.graphSha")}</dt>
                          <dd>
                            <code>{shortId(plan.graphSha256, 16)}</code>
                          </dd>
                        </div>
                        <div>
                          <dt>{t("director.plan.order")}</dt>
                          <dd>{graph.topologicalOrder.join(" → ")}</dd>
                        </div>
                        <div>
                          <dt>{t("director.participants.reviewers")}</dt>
                          <dd>{detail.participants.reviewerCount}</dd>
                        </div>
                        <div>
                          <dt>
                            {t(
                              "director.participants.independentReceiptsLabel",
                            )}
                          </dt>
                          <dd>
                            {detail.reviewerEvidence
                              ?.independentReviewerCount ?? 0}
                          </dd>
                        </div>
                      </dl>
                      <div className="director-reviewers">
                        <h3>{t("director.participants.requiredRoles")}</h3>
                        {!reviewerAssignmentsMatch(detail) && (
                          <p
                            className="ui-callout ui-callout--danger"
                            role="alert"
                          >
                            {t("director.participants.planMismatch")}
                          </p>
                        )}
                        {detail.participants.reviewerRoles.length > 0 ? (
                          <ul>
                            {detail.participants.reviewerRoles.map((role) => {
                              const assignment =
                                detail.participants.reviewerAssignments.find(
                                  (candidate) => candidate.role === role,
                                );
                              const receipt =
                                detail.reviewerEvidence?.receipts.find(
                                  (candidate) => candidate.role === role,
                                );
                              return (
                                <li key={role}>
                                  <Badge tone="info">
                                    {t(`director.reviewerRole.${role}`)}
                                  </Badge>{" "}
                                  <code>
                                    {assignment?.principalId ??
                                      t("director.participants.unassigned")}
                                  </code>{" "}
                                  {receipt ? (
                                    <>
                                      <Badge
                                        tone={reviewerReceiptTone(
                                          receipt.result,
                                        )}
                                      >
                                        {t(
                                          "director.participants.receiptStatus",
                                          {
                                            result: receipt.result,
                                            evidence: receipt.evidence,
                                          },
                                        )}
                                      </Badge>{" "}
                                      <span className="muted">
                                        {receipt.principalId}
                                      </span>
                                    </>
                                  ) : (
                                    <Badge tone="attention">
                                      {t(
                                        "director.participants.receiptMissing",
                                      )}
                                    </Badge>
                                  )}
                                </li>
                              );
                            })}
                          </ul>
                        ) : (
                          <p className="muted">
                            {t("director.participants.noRoles")}
                          </p>
                        )}
                        <p className="muted">
                          {t("director.participants.independentReceipts", {
                            count:
                              detail.reviewerEvidence
                                ?.independentReviewerCount ?? 0,
                            required: detail.participants.reviewerCount,
                          })}
                        </p>
                      </div>
                      {detail.participants.unitAssignments.length > 0 && (
                        <div className="director-unit-routing">
                          <h3>{t("director.participants.unitRouting")}</h3>
                          <ul>
                            {detail.participants.unitAssignments.map((unit) => (
                              <li key={unit.unitId}>
                                <code>{unit.unitId}</code> ·{" "}
                                {unit.specialization}
                                {unit.provider && unit.model
                                  ? ` · ${unit.provider}/${unit.model}`
                                  : ""}
                              </li>
                            ))}
                          </ul>
                        </div>
                      )}
                      <ul className="specs-units">
                        {graph.units.map((unit) => (
                          <li key={unit.id}>
                            <div className="specs-unit-heading">
                              <code>{unit.id}</code>
                              <strong>{unit.title}</strong>
                            </div>
                            <p className="muted">{unit.objective}</p>
                            <p className="muted">
                              {unit.paths.join(", ")}
                              {unit.dependsOn.length > 0
                                ? ` · ${t("specs.detail.dependsOn")}: ${unit.dependsOn.join(", ")}`
                                : ""}
                            </p>
                          </li>
                        ))}
                      </ul>
                    </>
                  ) : (
                    <p className="muted">{t("director.plan.empty")}</p>
                  )}
                  {assessmentError && (
                    <p className="ui-callout ui-callout--danger" role="alert">
                      {assessmentError}
                    </p>
                  )}
                  {assessmentNotice && (
                    <p className="notice" role="status">
                      {assessmentNotice}
                    </p>
                  )}
                  {detail.triage.route !== "NO_AGENT" &&
                    (!detail.assessmentReady ||
                      !detail.leadPlanReady ||
                      !detail.actionSpecReady) && (
                      <div
                        className="ui-callout ui-callout--attention"
                        role="status"
                      >
                        <p>
                          {!detail.assessmentReady
                            ? t("director.plan.assessmentRequired")
                            : !detail.leadPlanReady
                              ? t("director.plan.leadPlanRequired")
                              : t("director.plan.actionSpecRequired")}
                        </p>
                        {(!detail.assessmentReady || !detail.leadPlanReady) && (
                          <Button
                            variant="primary"
                            size="sm"
                            busy={assessmentBusy}
                            disabled={assessmentBusy}
                            onClick={() => void runLeadAssessment()}
                          >
                            {assessmentBusy
                              ? t("director.plan.assessing")
                              : t("director.plan.runAssessment")}
                          </Button>
                        )}
                      </div>
                    )}
                  <div className="director-priority">
                    <Button
                      variant="secondary"
                      size="sm"
                      busy={planBusy}
                      disabled={
                        planBusy ||
                        !detail.assessmentReady ||
                        !detail.leadPlanReady ||
                        !detail.actionSpecReady ||
                        detail.triage.route === "NO_AGENT" ||
                        Boolean(spec?.frozen || spec?.draft)
                      }
                      onClick={() => void compilePlan()}
                    >
                      {planBusy
                        ? t("director.plan.compiling")
                        : t("director.plan.compile")}
                    </Button>
                    <Button
                      variant="secondary"
                      size="sm"
                      busy={participantBusy}
                      disabled={participantBusy || !plan}
                      onClick={() => void recordParticipants()}
                    >
                      {participantBusy
                        ? t("director.participants.recording")
                        : t("director.participants.record")}
                    </Button>
                  </div>
                  {Boolean(spec?.frozen || spec?.draft) && (
                    <p className="muted">{t("director.plan.frozen")}</p>
                  )}
                </Panel>

                <Panel>
                  <div className="section-heading">
                    <h2>{t("director.planning.title")}</h2>
                    {planning?.review && (
                      <Badge
                        tone={
                          !reviewBinding.current
                            ? "attention"
                            : planning.review.status === "PENDING"
                              ? "attention"
                              : planning.review.status === "REJECTED" ||
                                  planning.review.status === "CHANGES_REQUESTED"
                                ? "danger"
                                : "success"
                        }
                      >
                        {reviewBinding.current
                          ? planning.review.status
                          : t("director.planning.staleBadge")}
                      </Badge>
                    )}
                  </div>
                  {planningError && (
                    <p className="ui-callout ui-callout--danger" role="alert">
                      {planningError}{" "}
                      <Button
                        size="sm"
                        variant="secondary"
                        onClick={() => void reloadDirector()}
                      >
                        {t("common.retry")}
                      </Button>
                    </p>
                  )}
                  {planningNotice && (
                    <p className="notice" role="status">
                      {planningNotice}
                    </p>
                  )}
                  <div className="director-preparation">
                    <h3>{t("director.preparation.title")}</h3>
                    {detail.request.mode === "AUTONOMOUS" &&
                      preparationPhase === "PREPARING" && (
                        <p className="muted">
                          {t("director.preparation.autopilot")}
                        </p>
                      )}
                    <dl className="director-facts director-facts--wide">
                      <div>
                        <dt>{t("director.preparation.artifacts")}</dt>
                        <dd>
                          {planning === null
                            ? planningError
                              ? t("director.preparation.unavailable")
                              : t("director.preparation.loading")
                            : artifactsPrepared
                              ? t("director.preparation.prepared")
                              : t("director.preparation.missing", {
                                  values: planning.missingArtifacts.join(", "),
                                })}
                        </dd>
                      </div>
                      <div>
                        <dt>{t("director.preparation.participants")}</dt>
                        <dd>
                          {planning === null
                            ? planningError
                              ? t("director.preparation.unavailable")
                              : t("director.preparation.loading")
                            : participantPlan === null
                              ? t("director.preparation.unavailable")
                              : !participantPlan.required
                                ? t("director.preparation.notRequired")
                                : participantsPrepared
                                  ? t("director.preparation.prepared")
                                  : participantPlan.present
                                    ? t("director.preparation.incomplete")
                                    : t("director.preparation.notRecorded")}
                        </dd>
                      </div>
                      <div>
                        <dt>{t("director.preparation.review")}</dt>
                        <dd>
                          {planning === null
                            ? planningError
                              ? t("director.preparation.unavailable")
                              : t("director.preparation.loading")
                            : planning.reviewApproved
                              ? t("director.preparation.approved")
                              : planning.review && !reviewBinding.current
                                ? t("director.preparation.staleReview")
                                : (reviewStatus ??
                                  t("director.preparation.notRecorded"))}
                        </dd>
                      </div>
                      <div>
                        <dt>{t("director.preparation.environment")}</dt>
                        <dd>
                          {environment === null
                            ? t("director.preparation.notRecorded")
                            : `${environmentStatusLabel(environment.status, t)}${
                                environment.blockers.length > 0
                                  ? ` · ${t("director.preparation.blockers", {
                                      count: environment.blockers.length,
                                    })}`
                                  : ""
                              }`}
                        </dd>
                      </div>
                    </dl>
                    {planning && (
                      <p
                        className={
                          preparationPhase === "READY"
                            ? "notice"
                            : preparationPhase === "PREPARING" ||
                                preparationPhase === "EXECUTING" ||
                                preparationPhase === "WAITING_OWNER" ||
                                preparationPhase === "TERMINAL"
                              ? "ui-callout ui-callout--info"
                              : "ui-callout ui-callout--danger"
                        }
                        role={
                          preparationPhase === "REVIEW_REJECTED" ||
                          preparationPhase === "RECOVERABLE_FAILURE"
                            ? "alert"
                            : "status"
                        }
                      >
                        {preparationPhaseMessage}
                      </p>
                    )}
                    {operationInconsistent && (
                      <p className="ui-callout ui-callout--danger" role="alert">
                        {t("director.preparation.operationInconsistent")}
                      </p>
                    )}
                    {startPollExhausted && (
                      <p
                        className="ui-callout ui-callout--attention"
                        role="status"
                      >
                        {t("director.preparation.startNotObserved")}
                      </p>
                    )}
                    {preparationStalled && (
                      <p
                        className="ui-callout ui-callout--attention"
                        role="status"
                      >
                        {t("director.preparation.pollingPaused")}
                      </p>
                    )}
                    {startReady && (
                      <p className="muted">
                        {t("director.preparation.startBoundary")}
                      </p>
                    )}
                    {planning?.ready &&
                      environment?.required === true &&
                      environment.status !== "FROZEN" &&
                      !environmentBlocked && (
                        <p className="muted">
                          {t("director.preparation.environmentBoundary")}
                        </p>
                      )}
                  </div>
                  {!planning ? (
                    <p className="muted">{t("director.planning.empty")}</p>
                  ) : (
                    <>
                      <dl className="director-facts">
                        <div>
                          <dt>{t("director.planning.lane")}</dt>
                          <dd>{planning.lane}</dd>
                        </div>
                        <div>
                          <dt>{t("director.planning.workflow")}</dt>
                          <dd>{planning.workflow?.state ?? "—"}</dd>
                        </div>
                        <div>
                          <dt>{t("director.planning.revision")}</dt>
                          <dd>{planning.review?.planRevision ?? "—"}</dd>
                        </div>
                        <div>
                          <dt>{t("director.planning.hash")}</dt>
                          <dd>
                            <code title={planning.review?.planSha256}>
                              {planning.review
                                ? shortId(planning.review.planSha256, 16)
                                : "—"}
                            </code>
                          </dd>
                        </div>
                        <div>
                          <dt>{t("director.planning.principal")}</dt>
                          <dd>{planning.review?.principalId ?? "—"}</dd>
                        </div>
                        <div>
                          <dt>{t("director.planning.points")}</dt>
                          <dd>{planning.review?.points ?? 0}</dd>
                        </div>
                      </dl>
                      {planning.missingArtifacts.length > 0 && (
                        <p className="ui-callout ui-callout--attention">
                          {t("director.planning.missing", {
                            values: planning.missingArtifacts.join(", "),
                          })}
                        </p>
                      )}
                      {planning.participantPlan.required &&
                        !planning.participantPlan.ready && (
                          <div
                            className="ui-callout ui-callout--attention"
                            role="status"
                          >
                            <p>
                              <strong>
                                {t(
                                  "director.planning.participantIncompleteTitle",
                                )}
                              </strong>{" "}
                              {planning.participantPlan.present
                                ? t("director.planning.participantIncomplete", {
                                    values:
                                      [
                                        ...planning.participantPlan
                                          .missingRoles,
                                        ...planning.participantPlan
                                          .unexpectedRoles,
                                        ...planning.participantPlan
                                          .missingAssignments,
                                      ].join(", ") ||
                                      t(
                                        "director.planning.participantAssignments",
                                      ),
                                  })
                                : t("director.planning.participantMissing")}
                            </p>
                            {planning.participantPlan.present &&
                              planning.participantPlan.problems.length > 0 && (
                                <ul className="muted">
                                  {planning.participantPlan.problems.map(
                                    (problem) => (
                                      <li key={problem}>{problem}</li>
                                    ),
                                  )}
                                </ul>
                              )}
                          </div>
                        )}
                      {!planning.reviewApproved && (
                        <p
                          className={`ui-callout ${
                            planning.review && !reviewBinding.current
                              ? "ui-callout--attention"
                              : reviewStatus === "PENDING"
                                ? "ui-callout--info"
                                : "ui-callout--attention"
                          }`}
                          role="status"
                        >
                          {planning.review && !reviewBinding.current
                            ? t("director.planning.staleReview")
                            : reviewStatus === "PENDING"
                              ? t("director.planning.reviewPending")
                              : reviewRevisitable && reviewDecisionAvailable
                                ? t("director.planning.reviewRevisit", {
                                    status: reviewStatus ?? "—",
                                  })
                                : t("director.planning.reviewNotApproved", {
                                    status: reviewStatus ?? "—",
                                  })}
                        </p>
                      )}
                      {planning.review && (
                        <p className="muted">
                          {t("director.planning.rationale", {
                            value: planning.review.rationale,
                          })}
                        </p>
                      )}
                      {planning.review && !reviewBinding.current && (
                        <p
                          className="ui-callout ui-callout--attention"
                          role="status"
                        >
                          {t("director.planning.staleReview")}
                        </p>
                      )}
                      {planning.review &&
                        reviewDecisionAvailable &&
                        (recordedDecision?.requestId === activeId ? (
                          <p
                            className="ui-callout ui-callout--info"
                            role="status"
                          >
                            {t("director.planning.decisionRecordedRefresh")}{" "}
                            <Button
                              size="sm"
                              variant="secondary"
                              onClick={() => void reloadDirector()}
                            >
                              {t("director.planning.refreshState")}
                            </Button>
                          </p>
                        ) : (
                          <div className="director-planning-review">
                            <Textarea
                              label={t("director.planning.reviewRationale")}
                              value={reviewRationale}
                              onChange={(event) =>
                                setReviewRationale(event.target.value)
                              }
                              help={
                                reviewRevisitable
                                  ? t("director.planning.revisitRationaleHelp")
                                  : undefined
                              }
                              disabled={activeReviewBusy !== null}
                            />
                            <div className="director-priority">
                              <Button
                                size="sm"
                                busy={activeReviewBusy === "APPROVE"}
                                disabled={
                                  activeReviewBusy !== null ||
                                  !planning.materialsReady
                                }
                                onClick={() => void reviewPlan("APPROVE")}
                              >
                                {t("director.planning.approve")}
                              </Button>
                              <Button
                                variant="secondary"
                                size="sm"
                                busy={activeReviewBusy === "CHANGES_REQUESTED"}
                                disabled={activeReviewBusy !== null}
                                onClick={() =>
                                  void reviewPlan("CHANGES_REQUESTED")
                                }
                              >
                                {t("director.planning.requestChanges")}
                              </Button>
                              <Button
                                variant="danger"
                                size="sm"
                                busy={activeReviewBusy === "REJECT"}
                                disabled={activeReviewBusy !== null}
                                onClick={() => void reviewPlan("REJECT")}
                              >
                                {t("director.planning.reject")}
                              </Button>
                            </div>
                          </div>
                        ))}
                      <dl className="director-facts director-facts--wide">
                        <div>
                          <dt>{t("director.planning.stack")}</dt>
                          <dd>
                            {artifact<{
                              selected: string;
                              source: string;
                              requiredCapabilities: string[];
                            }>("STACK")?.selected ?? "—"}{" "}
                            ·{" "}
                            {artifact<{ source: string }>("STACK")?.source ??
                              "—"}
                          </dd>
                        </div>
                        <div>
                          <dt>{t("director.planning.architecture")}</dt>
                          <dd>
                            {artifact<{ components: string[] }>("ARCHITECTURE")
                              ?.components.length ?? 0}{" "}
                            {t("director.planning.components")}
                          </dd>
                        </div>
                        <div>
                          <dt>{t("director.planning.documentation")}</dt>
                          <dd>
                            {artifact<{ required: unknown[] }>("DOCUMENTATION")
                              ?.required.length ?? 0}{" "}
                            {t("director.planning.requiredDocs")}
                          </dd>
                        </div>
                        <div>
                          <dt>{t("director.planning.skills")}</dt>
                          <dd>
                            {artifact<{ needs: string[] }>("SKILL")?.needs.join(
                              ", ",
                            ) ?? "—"}
                          </dd>
                        </div>
                        <div>
                          <dt>{t("director.planning.dod")}</dt>
                          <dd>
                            {artifact<{ projectChecks: unknown[] }>("DOD")
                              ?.projectChecks.length ?? 0}{" "}
                            {t("director.planning.checks")}
                          </dd>
                        </div>
                        <div>
                          <dt>{t("director.planning.openspec")}</dt>
                          <dd>
                            {artifact<{ changeId: string; revision: number }>(
                              "OPENSPEC",
                            )?.changeId ?? "—"}
                          </dd>
                        </div>
                      </dl>
                    </>
                  )}
                </Panel>

                <EnvironmentPanel
                  environment={detail.environment ?? null}
                  requestId={activeId}
                  executionBound={Boolean(detail.lead.campaignId)}
                  authoredSpecBound={Boolean(spec?.frozen)}
                  onEnvironmentUpdated={(next) =>
                    setDetail((current) =>
                      current ? { ...current, environment: next } : current,
                    )
                  }
                  onReviewed={() => void reloadDirector()}
                  onPlanRecompiled={() => void reloadDirector()}
                />

                <Panel>
                  <div className="section-heading">
                    <h2>{t("director.findings.title")}</h2>
                    <Badge
                      tone={
                        detail.findings.some(
                          (finding) => finding.status === "OPEN",
                        )
                          ? "attention"
                          : "success"
                      }
                    >
                      {t("director.findings.open", {
                        count: detail.findings.filter(
                          (finding) => finding.status === "OPEN",
                        ).length,
                      })}
                    </Badge>
                  </div>
                  {findingError && (
                    <p className="ui-callout ui-callout--danger" role="alert">
                      {findingError}
                    </p>
                  )}
                  {findingNotice && (
                    <p className="notice" role="status">
                      {findingNotice}
                    </p>
                  )}
                  {detail.findings.length === 0 ? (
                    <p className="muted">{t("director.findings.empty")}</p>
                  ) : (
                    <ul className="director-findings">
                      {detail.findings.map((finding) => (
                        <li key={finding.id}>
                          <div className="director-finding-head">
                            <code>{finding.findingId}</code>
                            <Badge
                              tone={
                                finding.status === "OPEN"
                                  ? "attention"
                                  : "success"
                              }
                            >
                              {finding.status}
                            </Badge>
                            <span className="muted">{finding.route}</span>
                          </div>
                          <p className="muted">
                            {t("director.findings.reviewer", {
                              value: finding.reviewerPrincipal,
                            })}
                          </p>
                          {finding.evidenceRefs.length > 0 && (
                            <p className="muted">
                              {t("director.findings.evidence", {
                                value: finding.evidenceRefs.join(", "),
                              })}
                            </p>
                          )}
                        </li>
                      ))}
                    </ul>
                  )}
                  {detail.findings.some(
                    (finding) => finding.status === "OPEN",
                  ) && (
                    <div className="director-finding-form">
                      <Select
                        label={t("director.findings.select")}
                        value={findingId}
                        onChange={(event) => setFindingId(event.target.value)}
                        disabled={findingBusy !== null}
                      >
                        <option value="">
                          {t("director.findings.selectPlaceholder")}
                        </option>
                        {detail.findings
                          .filter((finding) => finding.status === "OPEN")
                          .map((finding) => (
                            <option key={finding.id} value={finding.findingId}>
                              {finding.findingId} · {finding.route}
                            </option>
                          ))}
                      </Select>
                      <Textarea
                        label={t("director.findings.evidenceLabel")}
                        value={findingEvidence}
                        onChange={(event) =>
                          setFindingEvidence(event.target.value)
                        }
                        disabled={findingBusy !== null}
                      />
                      <Input
                        label={t("director.findings.ownerDecision")}
                        help={t("director.findings.ownerDecisionHelp")}
                        value={findingOwnerDecision}
                        onChange={(event) =>
                          setFindingOwnerDecision(event.target.value)
                        }
                        disabled={findingBusy !== null}
                      />
                      <div className="director-priority">
                        <Button
                          size="sm"
                          busy={findingBusy === "RESOLVED"}
                          disabled={findingBusy !== null || findingId === ""}
                          onClick={() => void disposeFinding("RESOLVED")}
                        >
                          {t("director.findings.resolve")}
                        </Button>
                        <Button
                          variant="danger"
                          size="sm"
                          busy={findingBusy === "REFUTED"}
                          disabled={
                            findingBusy !== null ||
                            findingId === "" ||
                            findingOwnerDecision.trim() === ""
                          }
                          onClick={() => void disposeFinding("REFUTED")}
                        >
                          {t("director.findings.refute")}
                        </Button>
                      </div>
                    </div>
                  )}
                </Panel>

                <Panel>
                  <div className="section-heading">
                    <h2>{t("director.spec.title")}</h2>
                    {spec?.frozen && (
                      <Badge tone="success">
                        {t("director.spec.frozen", {
                          revision: spec.frozen.record.revision,
                        })}
                      </Badge>
                    )}
                    {spec?.draft && (
                      <Badge tone="attention">
                        {t("director.spec.draft", {
                          revision: spec.draft.record.revision,
                        })}
                      </Badge>
                    )}
                  </div>
                  {specError && (
                    <p className="ui-callout ui-callout--danger" role="alert">
                      {specError}
                    </p>
                  )}
                  {specNotice && (
                    <p className="notice" role="status">
                      {specNotice}
                    </p>
                  )}
                  {detail.triage.route !== "FORMAL_SPEC" ? (
                    <p className="muted">{t("director.spec.notRequired")}</p>
                  ) : (
                    <>
                      {spec?.frozen?.spec && (
                        <>
                          <p>{spec.frozen.spec.summary}</p>
                          <p className="muted">
                            {t("director.spec.frozenNotice", {
                              revision: spec.frozen.record.revision,
                            })}
                          </p>
                          <Link
                            className="button button--secondary button--md"
                            to="/specs"
                          >
                            {t("director.spec.openBoard")}
                          </Link>
                        </>
                      )}
                      {spec?.draft?.preview && (
                        <div className="specs-draft">
                          <p>{spec.draft.preview.summary}</p>
                          <p className="muted">
                            {spec.draft.preview.requirements.length}{" "}
                            {t("specs.detail.traceability")}
                          </p>
                          <p className="muted">
                            <code>
                              {shortId(spec.draft.preview.specSha256, 16)}
                            </code>
                          </p>
                          <Button
                            size="sm"
                            busy={specBusy === "freeze"}
                            disabled={specBusy !== null}
                            onClick={() => void freezeSpec()}
                          >
                            {specBusy === "freeze"
                              ? t("director.spec.freezing")
                              : t("director.spec.freeze")}
                          </Button>
                        </div>
                      )}
                      {!spec?.frozen && !spec?.draft && (
                        <p className="muted">{t("director.spec.empty")}</p>
                      )}
                      <div className="director-priority">
                        <Button
                          variant="secondary"
                          size="sm"
                          busy={specBusy === "author"}
                          disabled={specBusy !== null || !plan}
                          onClick={() => void authorSpec()}
                        >
                          {specBusy === "author"
                            ? t("director.spec.authoring")
                            : t("director.spec.author")}
                        </Button>
                      </div>
                    </>
                  )}
                </Panel>

                <KnowledgePanel requestId={detail.request.id} />

                <Panel>
                  <div className="section-heading">
                    <h2>{t("director.semantic.title")}</h2>
                    {detail.semantic && (
                      <Badge tone="info">
                        {detail.semantic.provider}/{detail.semantic.model}
                      </Badge>
                    )}
                  </div>
                  {detail.semantic ? (
                    <>
                      <dl className="director-facts">
                        <div>
                          <dt>{t("director.semantic.recommendation")}</dt>
                          <dd>
                            {detail.semantic.recommendation.requestKind} ·{" "}
                            {detail.semantic.recommendation.route} ·{" "}
                            {detail.semantic.recommendation.assurance}
                          </dd>
                        </div>
                        <div>
                          <dt>{t("director.semantic.usage")}</dt>
                          <dd>
                            {t("director.semantic.usageValue", {
                              input: detail.semantic.usage.inputTokens,
                              output: detail.semantic.usage.outputTokens,
                              cost: detail.semantic.usage.costMicros,
                            })}
                          </dd>
                        </div>
                        <div>
                          <dt>{t("director.semantic.skills")}</dt>
                          <dd>
                            {detail.semantic.recommendation.skillNeeds.length
                              ? detail.semantic.recommendation.skillNeeds.join(
                                  ", ",
                                )
                              : t("director.semantic.none")}
                          </dd>
                        </div>
                      </dl>
                      {detail.semantic.recommendation.unknowns.length > 0 && (
                        <p className="ui-callout ui-callout--attention">
                          {t("director.detail.unknowns", {
                            values:
                              detail.semantic.recommendation.unknowns.join(
                                "; ",
                              ),
                          })}
                        </p>
                      )}
                    </>
                  ) : (
                    <p className="muted">{t("director.semantic.empty")}</p>
                  )}
                </Panel>

                <Panel>
                  <div className="section-heading">
                    <h2>{t("director.stack.title")}</h2>
                    <Badge tone="neutral">
                      {t("director.stack.confidence", {
                        value: detail.stack.confidence,
                      })}
                    </Badge>
                  </div>
                  <div className="director-chip-grid">
                    {(
                      [
                        ["director.stack.languages", detail.stack.languages],
                        ["director.stack.frameworks", detail.stack.frameworks],
                        ["director.stack.data", detail.stack.data],
                        ["director.stack.tests", detail.stack.testFrameworks],
                        ["director.stack.browser", detail.stack.browser],
                        ["director.stack.infra", detail.stack.infra],
                      ] as const
                    ).map(([label, values]) => (
                      <div className="director-chip-group" key={label}>
                        <span className="director-chip-label">{t(label)}</span>
                        <div className="director-chips">
                          {values.length === 0 ? (
                            <span className="muted">
                              {t("director.stack.none")}
                            </span>
                          ) : (
                            values.map((value) => (
                              <Badge key={value} tone="neutral">
                                {value}
                              </Badge>
                            ))
                          )}
                        </div>
                      </div>
                    ))}
                  </div>
                  {detail.stack.evidenceRefs.length > 0 && (
                    <p className="muted director-evidence">
                      {t("director.stack.evidence", {
                        values: detail.stack.evidenceRefs.join(", "),
                      })}
                    </p>
                  )}
                </Panel>

                <Panel>
                  <div className="section-heading">
                    <h2>{t("director.agents.title")}</h2>
                    <span className="muted">
                      {t("director.agents.count", {
                        count: detail.participants.assignments.reduce(
                          (sum, item) => sum + item.count,
                          0,
                        ),
                      })}
                    </span>
                  </div>
                  <div className="director-table-wrap">
                    <table className="director-table">
                      <thead>
                        <tr>
                          <th scope="col">{t("director.agents.role")}</th>
                          <th scope="col">
                            {t("director.agents.specialization")}
                          </th>
                          <th scope="col">{t("director.agents.count")}</th>
                          <th scope="col">{t("director.agents.access")}</th>
                          <th scope="col">{t("director.agents.tools")}</th>
                        </tr>
                      </thead>
                      <tbody>
                        {detail.participants.assignments.map((assignment) => (
                          <tr
                            key={`${assignment.role}-${assignment.specialization}`}
                          >
                            <th scope="row">{assignment.role}</th>
                            <td>{assignment.specialization}</td>
                            <td>{assignment.count}</td>
                            <td>
                              {assignment.readOnly
                                ? t("director.agents.readOnly")
                                : t("director.agents.write")}
                            </td>
                            <td>
                              <span className="muted">
                                {assignment.toolPack.join(", ")}
                              </span>
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                  <div className="director-context">
                    <h3>{t("director.context.title")}</h3>
                    <dl className="director-facts">
                      <div>
                        <dt>{t("director.context.compaction")}</dt>
                        <dd>
                          {t("director.context.compactionValue", {
                            percent:
                              detail.participants.contextPolicy
                                .compactAtPercent,
                            count:
                              detail.participants.contextPolicy
                                .maxConsecutiveCompactions,
                          })}
                        </dd>
                      </div>
                      <div>
                        <dt>{t("director.context.handoff")}</dt>
                        <dd>
                          {detail.participants.contextPolicy
                            .handoffAtStableMilestone
                            ? t("director.context.handoffEnabled")
                            : t("director.context.handoffDisabled")}
                        </dd>
                      </div>
                    </dl>
                    <p className="muted">{detail.context.note}</p>
                  </div>
                </Panel>

                <Panel>
                  <div className="section-heading">
                    <h2>{t("director.specs.title")}</h2>
                    <span className="muted">
                      {t("director.specs.count", {
                        count: detail.specifications.length,
                      })}
                    </span>
                  </div>
                  {detail.specifications.length === 0 ? (
                    <EmptyState
                      title={t("director.specs.emptyTitle")}
                      description={t("director.specs.emptyDescription")}
                    />
                  ) : (
                    <div className="director-table-wrap">
                      <table className="director-table">
                        <thead>
                          <tr>
                            <th scope="col">{t("director.specs.unit")}</th>
                            <th scope="col">
                              {t("director.specs.description")}
                            </th>
                            <th scope="col">{t("director.specs.status")}</th>
                            <th scope="col">{t("director.specs.paths")}</th>
                          </tr>
                        </thead>
                        <tbody>
                          {detail.specifications.map((spec) => (
                            <tr key={spec.id}>
                              <th scope="row">
                                <code>{spec.id}</code>
                              </th>
                              <td>{spec.description}</td>
                              <td>
                                <Badge tone="neutral">{spec.status}</Badge>
                              </td>
                              <td>
                                <span className="muted">
                                  {spec.allowedPaths.join(", ")}
                                </span>
                              </td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  )}
                </Panel>
              </>
            )}
            {!detailLoading && !detailError && !detail && selectedEntry && (
              <EmptyState
                title={t("director.detail.emptyTitle")}
                description={t("director.detail.emptyDescription")}
              />
            )}
          </div>
        </div>
      )}
    </div>
  );
}
