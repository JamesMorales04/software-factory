package lead

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"testing"
)

func loadEvaluationCorpus(t *testing.T) EvaluationCorpus {
	t.Helper()
	raw, err := os.ReadFile("testdata/s4d-evaluation-corpus.json")
	if err != nil {
		t.Fatal(err)
	}
	var corpus EvaluationCorpus
	if err := json.Unmarshal(raw, &corpus); err != nil {
		t.Fatal(err)
	}
	if len(corpus.Fixtures) != 11 {
		t.Fatalf("corpus fixtures = %d", len(corpus.Fixtures))
	}
	return corpus
}

func fullSheet(t *testing.T, corpus EvaluationCorpus, system string, score int) EvaluationScoreSheet {
	t.Helper()
	sheet := EvaluationScoreSheet{System: system}
	for _, fixture := range corpus.Fixtures {
		for _, dimension := range FixtureDimensions(fixture) {
			sheet.Entries = append(sheet.Entries, EvaluationScoreEntry{
				FixtureID: fixture.ID, Dimension: dimension, Score: score,
			})
		}
	}
	return sheet
}

func TestSemanticEvaluationScoresAndComparesBlindedSheets(t *testing.T) {
	corpus := loadEvaluationCorpus(t)
	model, err := Evaluate(corpus, fullSheet(t, corpus, "model", 2))
	if err != nil {
		t.Fatal(err)
	}
	baseline, err := Evaluate(corpus, fullSheet(t, corpus, "baseline", 1))
	if err != nil {
		t.Fatal(err)
	}
	if model.Overall != 100 || baseline.Overall != 50 {
		t.Fatalf("overall scores model=%v baseline=%v", model.Overall, baseline.Overall)
	}
	comparison, err := Compare(corpus, model, baseline)
	if err != nil {
		t.Fatal(err)
	}
	if !comparison.Pass || comparison.MeanDifference != 50 {
		t.Fatalf("comparison = %+v", comparison)
	}
}

func TestSemanticEvaluationFailsClosedOnMissingScoresAndThresholds(t *testing.T) {
	corpus := loadEvaluationCorpus(t)
	modelSheet := fullSheet(t, corpus, "model", 2)
	baselineSheet := fullSheet(t, corpus, "baseline", 1)
	modelSheet.Entries = modelSheet.Entries[1:]
	if _, err := Evaluate(corpus, modelSheet); err == nil {
		t.Fatal("missing score was accepted")
	}
	model, _ := Evaluate(corpus, fullSheet(t, corpus, "model", 2))
	baseline, _ := Evaluate(corpus, baselineSheet)
	model.Fixtures[0].Critical = true
	model.Critical = []string{"S4D-01: unsupported completion claim"}
	comparison, err := Compare(corpus, model, baseline)
	if err != nil || comparison.Pass {
		t.Fatalf("critical failure did not fail comparison: %+v err=%v", comparison, err)
	}
	weak, _ := Evaluate(corpus, fullSheet(t, corpus, "weak", 0))
	comparison, err = Compare(corpus, weak, baseline)
	if err != nil || comparison.Pass {
		t.Fatalf("zero-score model passed: %+v err=%v", comparison, err)
	}
}

func TestSemanticEvaluationDoesNotRequireInapplicableDimensions(t *testing.T) {
	corpus := loadEvaluationCorpus(t)
	sheet := fullSheet(t, corpus, "model", 2)
	for _, entry := range sheet.Entries {
		for _, fixture := range corpus.Fixtures {
			if fixture.ID == entry.FixtureID && !fixture.Estimate && entry.Dimension == DimensionEffort {
				t.Fatalf("effort was required for non-estimate fixture %s", fixture.ID)
			}
		}
	}
	if _, err := Evaluate(corpus, sheet); err != nil {
		t.Fatal(err)
	}
}

func TestSemanticEvaluationCorpusIsFrozen(t *testing.T) {
	raw, err := os.ReadFile("testdata/s4d-evaluation-corpus.json")
	if err != nil {
		t.Fatal(err)
	}
	sum := sha256.Sum256(raw)
	const frozen = "5133ebd92c387a7d12ed2ffe937dcf1f5af751c5d0101267c6881942fc692d1b"
	if hex.EncodeToString(sum[:]) != frozen {
		t.Fatal("CAP-014 corpus changed without a new frozen revision")
	}
}

func TestSemanticEvaluationRejectsUnknownFixtureIDs(t *testing.T) {
	corpus := loadEvaluationCorpus(t)
	sheet := fullSheet(t, corpus, "model", 2)
	sheet.Entries = append(sheet.Entries, EvaluationScoreEntry{
		FixtureID: "S4D-99", Dimension: DimensionIntentRoute, Score: 2,
	})
	if _, err := Evaluate(corpus, sheet); err == nil {
		t.Fatal("unknown fixture ID was silently accepted")
	}
}
