package quality

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"sort"
	"strings"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

// Input is the complete durable evidence set for one candidate. The gate
// reads no other state and has no side effects.
type Input struct {
	Candidate     domain.CandidateRevision
	Spec          domain.ActionSpec
	Run           domain.Run
	Attempt       domain.Attempt
	Verifications []domain.VerificationEvidence
	Reviews       []domain.ReviewEvidence
}

// Evaluation is a gate result plus whether it is final. A non-final result
// (missing independent evidence) is a truthful NOT_TESTED projection and is
// never persisted as an authoritative decision.
type Evaluation struct {
	Decision domain.GateDecision
	Final    bool
}

// Evaluate deterministically decides one candidate. Every reason and check
// is derived from durable identities and concrete results; no model output
// participates.
func Evaluate(in Input) Evaluation {
	checks := make([]domain.CheckResult, 0, 8+len(in.Spec.AllChecks()))
	reasons := make([]string, 0)
	decision := domain.GateDecision{
		CandidateID:    in.Candidate.ID,
		CandidateSHA:   in.Candidate.CommitSHA,
		SpecID:         in.Candidate.SpecID,
		SpecHash:       in.Candidate.SpecHash,
		PolicyRevision: domain.QualityPolicyRevision,
		Actor:          "core-quality-gate",
	}

	identityOK, identityDetail := identityCheck(in)
	checks = append(checks, gateCheck("gate-identity", domain.CheckIdentity, identityOK, identityDetail))
	if !identityOK {
		reasons = append(reasons, "identity: "+identityDetail)
	}

	scopeOK, scopeDetail := scopeCheck(in)
	checks = append(checks, gateCheck("gate-scope", domain.CheckScope, scopeOK, scopeDetail))
	if !scopeOK {
		reasons = append(reasons, "scope: "+scopeDetail)
	}

	freshOK, freshDetail := freshnessCheck(in)
	checks = append(checks, gateCheck("gate-freshness", domain.CheckFreshness, freshOK, freshDetail))
	if !freshOK {
		reasons = append(reasons, "freshness: "+freshDetail)
	}

	independenceOK, independenceDetail := independenceCheck(in)
	checks = append(checks, gateCheck("gate-independence", domain.CheckIndependence, independenceOK, independenceDetail))
	if !independenceOK {
		reasons = append(reasons, "independence: "+independenceDetail)
	}

	riskClass := domain.RiskClass("R0")
	if risk := ClassifyChange(in.Candidate.ChangedPaths); domain.RiskClass(risk.RiskClass) != "" {
		riskClass = domain.RiskClass(risk.RiskClass)
	}
	securityOK := riskRank(string(riskClass)) < 2 && riskRank(string(in.Spec.RiskClass)) >= riskRank(string(riskClass))
	securityDetail := "final diff risk " + string(riskClass) + " within spec risk " + string(in.Spec.RiskClass)
	if !securityOK {
		switch {
		case riskRank(string(riskClass)) >= 2:
			securityDetail = "final diff risk " + string(riskClass) + " requires protected human approval"
		default:
			securityDetail = "spec risk " + string(in.Spec.RiskClass) + " under-classifies final diff risk " + string(riskClass)
		}
	}
	checks = append(checks, gateCheck("gate-security", domain.CheckSecurity, securityOK, securityDetail))
	if !securityOK {
		reasons = append(reasons, "security: "+securityDetail)
	}

	checkResults, checkReasons := verifierChecks(in)
	checks = append(checks, checkResults...)
	reasons = append(reasons, checkReasons...)

	reviewResult, reviewReasons := reviewCheck(in)
	checks = append(checks, reviewResult)
	reasons = append(reasons, reviewReasons...)

	decision.Checks = checks
	decision.Reasons = boundedReasons(reasons)
	decision.EvidenceDigest = EvidenceDigest(in.Verifications, in.Reviews)

	final := len(in.Verifications) > 0 && len(in.Reviews) > 0
	if !final {
		decision.Status = domain.GateNotTested
		return Evaluation{Decision: decision, Final: false}
	}

	decision.Status = aggregateStatus(checks, riskRank(string(riskClass)))
	return Evaluation{Decision: decision, Final: true}
}

// EvidenceDigest binds one decision to the exact durable evidence set that
// was evaluated. It prevents a stale evaluation from persisting after new
// evidence committed concurrently.
func EvidenceDigest(verifications []domain.VerificationEvidence, reviews []domain.ReviewEvidence) string {
	verificationIDs := make([]string, 0, len(verifications))
	for _, evidence := range verifications {
		verificationIDs = append(verificationIDs, evidence.ID)
	}
	reviewIDs := make([]string, 0, len(reviews))
	for _, review := range reviews {
		reviewIDs = append(reviewIDs, review.ID)
	}
	return EvidenceIDDigest(verificationIDs, reviewIDs)
}

// EvidenceIDDigest is the identity-only form used by the store to verify
// that the evaluated set is still the committed set at save time.
func EvidenceIDDigest(verificationIDs, reviewIDs []string) string {
	sortedVerifications := append([]string(nil), verificationIDs...)
	sortedReviews := append([]string(nil), reviewIDs...)
	sort.Strings(sortedVerifications)
	sort.Strings(sortedReviews)
	payload := "v:" + strings.Join(sortedVerifications, ",") + "|r:" + strings.Join(sortedReviews, ",")
	sum := sha256.Sum256([]byte(payload))
	return hex.EncodeToString(sum[:])
}

func identityCheck(in Input) (bool, string) {
	c := in.Candidate
	if err := c.Validate(); err != nil {
		return false, "candidate row failed validation: " + err.Error()
	}
	if err := in.Spec.Validate(); err != nil {
		return false, "frozen spec failed validation: " + err.Error()
	}
	if c.State != domain.CandidateFrozen {
		return false, "candidate is " + string(c.State)
	}
	if in.Spec.PolicyRevision != domain.QualityPolicyRevision ||
		c.PolicyRevision != domain.QualityPolicyRevision {
		return false, "candidate/spec policy is not the active protected policy"
	}
	if in.Spec.ID != c.SpecID || in.Spec.ContentHash != c.SpecHash ||
		in.Spec.PolicyRevision != c.PolicyRevision || in.Spec.ActionID != c.ActionID ||
		in.Spec.ProjectID != c.ProjectID {
		return false, "candidate does not match the frozen specification"
	}
	if in.Run.ID != c.ImplementerRunID || in.Run.Status != domain.RunSucceeded {
		return false, "implementer Run is not the succeeded candidate Run"
	}
	if in.Run.SpecID != c.SpecID || in.Run.ExecutionMode != c.ExecutionMode ||
		in.Run.Provider != c.Provider || in.Run.Model != c.Model || in.Run.PaseoMode != c.PaseoMode {
		return false, "implementer Run routing/spec does not match the candidate"
	}
	if in.Attempt.ID != c.ImplementerAttemptID || in.Attempt.Status != domain.AttemptSucceeded ||
		in.Attempt.LeaseGeneration != c.ImplementerGeneration || in.Attempt.WorkerID != c.ImplementerWorkerID {
		return false, "implementer Attempt identity does not match the candidate"
	}
	return true, "exact candidate/spec/run/attempt identity"
}

func scopeCheck(in Input) (bool, string) {
	if len(in.Candidate.ChangedPaths) == 0 {
		return false, "candidate has no changed paths"
	}
	for _, changed := range in.Candidate.ChangedPaths {
		if !domain.PathAllowed(changed, in.Spec.AllowedPaths) {
			return false, "changed path outside authorized scope: " + changed
		}
	}
	risk := ClassifyChange(in.Candidate.ChangedPaths)
	if domain.RiskClass(risk.RiskClass) != in.Candidate.RiskClass {
		return false, "recorded risk " + string(in.Candidate.RiskClass) + " differs from recomputed " + risk.RiskClass
	}
	return true, "all changed paths are within the authorized scope"
}

func freshnessCheck(in Input) (bool, string) {
	if in.Candidate.FrozenAt.Before(in.Spec.FrozenAt) {
		return false, "candidate was frozen before the specification"
	}
	if in.Candidate.SupersededAt.After(in.Candidate.FrozenAt) && !in.Candidate.SupersededAt.IsZero() {
		return false, "candidate is superseded"
	}
	for _, evidence := range in.Verifications {
		if evidence.CreatedAt.Before(in.Candidate.FrozenAt) {
			return false, "verifier evidence predates the candidate"
		}
	}
	for _, review := range in.Reviews {
		if review.CreatedAt.Before(in.Candidate.FrozenAt) {
			return false, "review predates the candidate"
		}
	}
	return true, "all evidence postdates the frozen candidate"
}

func independenceCheck(in Input) (bool, string) {
	if len(in.Verifications) == 0 {
		return false, "no independent verifier evidence"
	}
	for _, evidence := range in.Verifications {
		if evidence.Authority != "verifier" || !evidence.Independent ||
			evidence.Actor == "" || evidence.Actor == in.Candidate.ImplementerWorkerID {
			return false, "verifier evidence is not independent"
		}
		if evidence.CandidateSHA != in.Candidate.CommitSHA || evidence.SpecID != in.Candidate.SpecID ||
			evidence.SpecHash != in.Candidate.SpecHash || evidence.PolicyRevision != in.Candidate.PolicyRevision ||
			evidence.OracleDigest != in.Spec.OracleDigest() {
			return false, "verifier evidence identity/oracle mismatch"
		}
	}
	if len(in.Reviews) == 0 {
		return false, "no independent reviewer verdict"
	}
	for _, review := range in.Reviews {
		if review.Authority != "reviewer" || review.Actor == "" ||
			review.Actor == in.Candidate.ImplementerWorkerID {
			return false, "reviewer evidence is not independent"
		}
		if review.CandidateSHA != in.Candidate.CommitSHA || review.SpecID != in.Candidate.SpecID ||
			review.SpecHash != in.Candidate.SpecHash || review.PolicyRevision != in.Candidate.PolicyRevision {
			return false, "reviewer evidence identity mismatch"
		}
	}
	return true, "separate verifier and reviewer authorities bound to the exact candidate"
}

func verifierChecks(in Input) ([]domain.CheckResult, []string) {
	var results []domain.CheckResult
	var reasons []string
	byID := map[string][]domain.CheckStatus{}
	for _, evidence := range in.Verifications {
		for _, check := range evidence.Checks {
			byID[check.ID] = append(byID[check.ID], check.Status)
		}
	}
	for _, expected := range in.Spec.AllChecks() {
		statuses, ok := byID[expected.ID]
		result := domain.CheckResult{
			ID: expected.ID, Kind: expected.Kind, Command: expected.Command,
		}
		if !ok || len(statuses) == 0 {
			result.Status = domain.CheckNotTested
			result.Detail = "verifier did not report this required check"
			reasons = append(reasons, "missing check: "+expected.ID)
			results = append(results, result)
			continue
		}
		result.Status = aggregateCheckStatus(statuses)
		switch result.Status {
		case domain.CheckPass:
			result.Detail = "verified PASS"
		case domain.CheckFail:
			result.Detail = "verified FAIL"
			reasons = append(reasons, "failing check: "+expected.ID)
		case domain.CheckNotTested:
			result.Detail = "verifier could not test this check"
			reasons = append(reasons, "not tested: "+expected.ID)
		case domain.CheckSkipped:
			result.Detail = "verifier skipped this required check"
			reasons = append(reasons, "skipped required check: "+expected.ID)
		case domain.CheckNotApplicable:
			result.Detail = "verifier marked this required check NOT_APPLICABLE"
			reasons = append(reasons, "not applicable required check: "+expected.ID)
		}
		results = append(results, result)
	}
	return results, reasons
}

func aggregateCheckStatus(statuses []domain.CheckStatus) domain.CheckStatus {
	worst := domain.CheckPass
	for _, status := range statuses {
		switch status {
		case domain.CheckFail:
			return domain.CheckFail
		case domain.CheckNotTested:
			worst = domain.CheckNotTested
		case domain.CheckSkipped, domain.CheckNotApplicable:
			if worst == domain.CheckPass {
				worst = status
			}
		}
	}
	return worst
}

func reviewCheck(in Input) (domain.CheckResult, []string) {
	result := domain.CheckResult{ID: "gate-review", Kind: domain.CheckReview}
	if len(in.Reviews) == 0 {
		result.Status = domain.CheckNotTested
		result.Detail = "no reviewer verdict"
		return result, []string{"missing reviewer verdict"}
	}
	approve := 0
	for _, review := range in.Reviews {
		switch review.Verdict {
		case domain.ReviewBlocked:
			result.Status = domain.CheckFail
			result.Detail = "reviewer BLOCKED the candidate"
			return result, []string{"reviewer blocked: " + strings.Join(findingMessages(review.Findings), "; ")}
		case domain.ReviewRequestChanges:
			result.Status = domain.CheckFail
			result.Detail = "reviewer requested changes"
			return result, []string{"review requested changes: " + strings.Join(findingMessages(review.Findings), "; ")}
		case domain.ReviewApprove:
			approve++
		default:
			result.Status = domain.CheckFail
			result.Detail = "unknown reviewer verdict"
			return result, []string{"unknown reviewer verdict"}
		}
	}
	if approve != len(in.Reviews) {
		result.Status = domain.CheckFail
		result.Detail = "reviewer verdicts do not agree"
		return result, []string{"reviewer verdicts do not agree"}
	}
	result.Status = domain.CheckPass
	result.Detail = "independent reviewer APPROVE"
	return result, nil
}

func findingMessages(findings []domain.ReviewFinding) []string {
	out := make([]string, 0, len(findings))
	for _, finding := range findings {
		if len(out) >= 4 {
			break
		}
		out = append(out, finding.Message)
	}
	if len(out) == 0 {
		return []string{"no bounded finding detail"}
	}
	return out
}

func gateCheck(id string, kind domain.CheckKind, ok bool, detail string) domain.CheckResult {
	status := domain.CheckPass
	if !ok {
		status = domain.CheckFail
	}
	return domain.CheckResult{ID: id, Kind: kind, Status: status, Detail: detail}
}

func aggregateStatus(checks []domain.CheckResult, finalRisk int) domain.GateStatus {
	human := false
	blocked := false
	repair := false
	notTested := false
	for _, check := range checks {
		switch check.Status {
		case domain.CheckFail:
			switch check.Kind {
			case domain.CheckSecurity:
				if finalRisk >= 2 || strings.Contains(check.Detail, "under-classifies") {
					human = true
				} else {
					blocked = true
				}
			case domain.CheckIdentity, domain.CheckScope, domain.CheckIndependence, domain.CheckFreshness:
				blocked = true
			case domain.CheckReview:
				if strings.Contains(check.Detail, "reviewer BLOCKED") {
					blocked = true
				} else {
					repair = true
				}
			case domain.CheckFormat, domain.CheckLint, domain.CheckBuild,
				domain.CheckTest, domain.CheckFunctional, domain.CheckNegative, domain.CheckBrowser:
				repair = true
			default:
				blocked = true
			}
		case domain.CheckNotTested:
			notTested = true
		case domain.CheckSkipped, domain.CheckNotApplicable:
			blocked = true
		}
	}
	switch {
	case human:
		return domain.GateHumanRequired
	case blocked:
		return domain.GateBlocked
	case repair:
		return domain.GateRepair
	case notTested:
		return domain.GateNotTested
	default:
		return domain.GateAccepted
	}
}

func boundedReasons(reasons []string) []string {
	seen := map[string]bool{}
	out := make([]string, 0, len(reasons))
	for _, reason := range reasons {
		reason = strings.TrimSpace(reason)
		if reason == "" || seen[reason] {
			continue
		}
		if len(reason) > 300 {
			reason = reason[:300]
		}
		seen[reason] = true
		out = append(out, reason)
		if len(out) >= 20 {
			break
		}
	}
	sort.Strings(out)
	return out
}

// Summary renders a bounded operator/log summary without evidence bodies.
func Summary(decision domain.GateDecision) string {
	return fmt.Sprintf("%s candidate=%s risk-checks=%d reasons=%d",
		decision.Status, decision.CandidateID, len(decision.Checks), len(decision.Reasons))
}
