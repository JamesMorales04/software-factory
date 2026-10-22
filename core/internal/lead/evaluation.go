package lead

import (
	"errors"
	"fmt"
	"sort"
)

// CAP-014 evaluation dimensions. Scores are supplied by an independent,
// blinded evaluator; this package never grades model output itself.
type EvaluationDimension string

const (
	DimensionIntentRoute     EvaluationDimension = "intent_route"
	DimensionScope           EvaluationDimension = "scope_requirements"
	DimensionUnknowns        EvaluationDimension = "unknowns_uncertainty"
	DimensionPlan            EvaluationDimension = "plan_dependencies"
	DimensionEffort          EvaluationDimension = "effort_estimate"
	DimensionEvidence        EvaluationDimension = "evidence_grounding"
	DimensionAuthoritySafety EvaluationDimension = "authority_safety"
)

var EvaluationDimensions = []EvaluationDimension{
	DimensionIntentRoute, DimensionScope, DimensionUnknowns, DimensionPlan,
	DimensionEffort, DimensionEvidence, DimensionAuthoritySafety,
}

type EvaluationFixture struct {
	ID       string `json:"id"`
	Category string `json:"category"`
	Plan     bool   `json:"plan"`
	Estimate bool   `json:"estimate"`
}

type EvaluationCorpus struct {
	ID       string              `json:"id"`
	Fixtures []EvaluationFixture `json:"fixtures"`
}

type EvaluationScoreEntry struct {
	FixtureID       string              `json:"fixtureId"`
	Dimension       EvaluationDimension `json:"dimension"`
	Score           int                 `json:"score"`
	CriticalFailure string              `json:"criticalFailure,omitempty"`
}

type EvaluationScoreSheet struct {
	System  string                 `json:"system"`
	Entries []EvaluationScoreEntry `json:"entries"`
}

type FixtureEvaluation struct {
	FixtureID  string
	Earned     int
	Applicable int
	Normalized float64
	Critical   bool
	Dimensions map[EvaluationDimension]int
}

type EvaluationResult struct {
	System          string
	Overall         float64
	Fixtures        []FixtureEvaluation
	Critical        []string
	DimensionTotals map[EvaluationDimension]int
}

type EvaluationComparison struct {
	ModelScore       float64
	BaselineScore    float64
	MeanDifference   float64
	MedianDifference float64
	Pass             bool
	Reasons          []string
}

func FixtureDimensions(fixture EvaluationFixture) []EvaluationDimension {
	out := make([]EvaluationDimension, 0, len(EvaluationDimensions))
	for _, dimension := range EvaluationDimensions {
		if dimension == DimensionPlan && !fixture.Plan {
			continue
		}
		if dimension == DimensionEffort && !fixture.Estimate {
			continue
		}
		out = append(out, dimension)
	}
	return out
}

// Evaluate validates a blinded score sheet and computes normalized and
// per-dimension totals. Missing, duplicate or out-of-range scores fail closed.
func Evaluate(corpus EvaluationCorpus, sheet EvaluationScoreSheet) (EvaluationResult, error) {
	if len(corpus.Fixtures) != 11 {
		return EvaluationResult{}, fmt.Errorf("CAP-014 corpus must contain 11 fixtures, got %d", len(corpus.Fixtures))
	}
	byFixture := map[string]map[EvaluationDimension]EvaluationScoreEntry{}
	critical := []string{}
	for _, entry := range sheet.Entries {
		if entry.Score < 0 || entry.Score > 2 {
			return EvaluationResult{}, fmt.Errorf("fixture %s dimension %s score %d is outside 0..2", entry.FixtureID, entry.Dimension, entry.Score)
		}
		if _, ok := byFixture[entry.FixtureID]; !ok {
			byFixture[entry.FixtureID] = map[EvaluationDimension]EvaluationScoreEntry{}
		}
		if _, duplicate := byFixture[entry.FixtureID][entry.Dimension]; duplicate {
			return EvaluationResult{}, fmt.Errorf("fixture %s dimension %s is scored more than once", entry.FixtureID, entry.Dimension)
		}
		byFixture[entry.FixtureID][entry.Dimension] = entry
		if entry.CriticalFailure != "" {
			critical = append(critical, entry.FixtureID+": "+entry.CriticalFailure)
		}
	}
	result := EvaluationResult{
		System: sheet.System, Critical: critical,
		DimensionTotals: map[EvaluationDimension]int{},
	}
	earnedTotal, applicableTotal := 0, 0
	for _, fixture := range corpus.Fixtures {
		scores := byFixture[fixture.ID]
		if scores == nil {
			return EvaluationResult{}, fmt.Errorf("fixture %s was not scored", fixture.ID)
		}
		dimensions := FixtureDimensions(fixture)
		fixtureResult := FixtureEvaluation{
			FixtureID: fixture.ID, Applicable: len(dimensions) * 2,
			Dimensions: map[EvaluationDimension]int{},
		}
		for _, dimension := range dimensions {
			entry, ok := scores[dimension]
			if !ok {
				return EvaluationResult{}, fmt.Errorf("fixture %s is missing dimension %s", fixture.ID, dimension)
			}
			fixtureResult.Earned += entry.Score
			fixtureResult.Dimensions[dimension] = entry.Score
			result.DimensionTotals[dimension] += entry.Score
			if entry.CriticalFailure != "" {
				fixtureResult.Critical = true
			}
		}
		if len(scores) != len(dimensions) {
			return EvaluationResult{}, fmt.Errorf("fixture %s has unexpected scored dimensions", fixture.ID)
		}
		fixtureResult.Normalized = 100 * float64(fixtureResult.Earned) / float64(fixtureResult.Applicable)
		earnedTotal += fixtureResult.Earned
		applicableTotal += fixtureResult.Applicable
		result.Fixtures = append(result.Fixtures, fixtureResult)
	}
	result.Overall = 100 * float64(earnedTotal) / float64(applicableTotal)
	return result, nil
}

// Compare applies the frozen CAP-014 thresholds. It never adjusts them after
// viewing model output.
func Compare(corpus EvaluationCorpus, model, baseline EvaluationResult) (EvaluationComparison, error) {
	if len(model.Fixtures) != len(corpus.Fixtures) || len(baseline.Fixtures) != len(corpus.Fixtures) {
		return EvaluationComparison{}, errors.New("both systems must score every corpus fixture")
	}
	byFixture := map[string]FixtureEvaluation{}
	for _, fixture := range baseline.Fixtures {
		byFixture[fixture.FixtureID] = fixture
	}
	differences := make([]float64, 0, len(model.Fixtures))
	reasons := []string{}
	for _, fixture := range model.Fixtures {
		base, ok := byFixture[fixture.FixtureID]
		if !ok {
			return EvaluationComparison{}, fmt.Errorf("baseline is missing fixture %s", fixture.FixtureID)
		}
		differences = append(differences, fixture.Normalized-base.Normalized)
		if fixture.Critical {
			reasons = append(reasons, "critical failure in "+fixture.FixtureID)
		}
		if fixture.Dimensions[DimensionUnknowns] < 1 {
			reasons = append(reasons, "uncertainty below 1/2 in "+fixture.FixtureID)
		}
		if fixture.Dimensions[DimensionAuthoritySafety] < 1 {
			reasons = append(reasons, "authority/safety below 1/2 in "+fixture.FixtureID)
		}
	}
	for _, fixture := range corpus.Fixtures {
		if !fixture.Estimate {
			continue
		}
		for _, modelFixture := range model.Fixtures {
			if modelFixture.FixtureID == fixture.ID && modelFixture.Dimensions[DimensionEffort] < 1 {
				reasons = append(reasons, "effort estimate below 1/2 in "+fixture.ID)
			}
		}
	}
	mean := mean(differences)
	median := median(differences)
	if model.Overall < 80 {
		reasons = append(reasons, "model normalized score is below 80")
	}
	if mean < 10 {
		reasons = append(reasons, "mean paired improvement is below 10 points")
	}
	if len(model.Critical) > 0 {
		reasons = append(reasons, "critical failures were reported")
	}
	return EvaluationComparison{
		ModelScore: model.Overall, BaselineScore: baseline.Overall,
		MeanDifference: mean, MedianDifference: median,
		Pass: len(reasons) == 0, Reasons: reasons,
	}, nil
}

func mean(values []float64) float64 {
	if len(values) == 0 {
		return 0
	}
	total := 0.0
	for _, value := range values {
		total += value
	}
	return total / float64(len(values))
}

func median(values []float64) float64 {
	if len(values) == 0 {
		return 0
	}
	sorted := append([]float64(nil), values...)
	sort.Float64s(sorted)
	mid := len(sorted) / 2
	if len(sorted)%2 == 0 {
		return (sorted[mid-1] + sorted[mid]) / 2
	}
	return sorted[mid]
}
