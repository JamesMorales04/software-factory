// s4d-evaluate applies the frozen CAP-014 rubric to independent, blinded
// score sheets. It never calls a model or provider.
package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"os"

	"github.com/JamesMorales04/software-factory/core/internal/lead"
)

func load(path string, target any) error {
	raw, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	return json.Unmarshal(raw, target)
}

func main() {
	corpusPath := flag.String("corpus", "core/internal/lead/testdata/s4d-evaluation-corpus.json", "frozen evaluation corpus")
	modelPath := flag.String("model", "", "blinded model score sheet")
	baselinePath := flag.String("baseline", "", "blinded baseline score sheet")
	flag.Parse()
	if *modelPath == "" || *baselinePath == "" {
		fmt.Fprintln(os.Stderr, "both -model and -baseline score sheets are required; no model is invoked")
		os.Exit(2)
	}
	var corpus lead.EvaluationCorpus
	var modelSheet, baselineSheet lead.EvaluationScoreSheet
	if err := load(*corpusPath, &corpus); err != nil {
		fmt.Fprintln(os.Stderr, "load corpus:", err)
		os.Exit(2)
	}
	if err := load(*modelPath, &modelSheet); err != nil {
		fmt.Fprintln(os.Stderr, "load model sheet:", err)
		os.Exit(2)
	}
	if err := load(*baselinePath, &baselineSheet); err != nil {
		fmt.Fprintln(os.Stderr, "load baseline sheet:", err)
		os.Exit(2)
	}
	model, err := lead.Evaluate(corpus, modelSheet)
	if err != nil {
		fmt.Fprintln(os.Stderr, "evaluate model:", err)
		os.Exit(2)
	}
	baseline, err := lead.Evaluate(corpus, baselineSheet)
	if err != nil {
		fmt.Fprintln(os.Stderr, "evaluate baseline:", err)
		os.Exit(2)
	}
	comparison, err := lead.Compare(corpus, model, baseline)
	if err != nil {
		fmt.Fprintln(os.Stderr, "compare:", err)
		os.Exit(2)
	}
	output := map[string]any{
		"corpus": corpus.ID, "model": model, "baseline": baseline, "comparison": comparison,
	}
	encoder := json.NewEncoder(os.Stdout)
	encoder.SetIndent("", "  ")
	if err := encoder.Encode(output); err != nil {
		fmt.Fprintln(os.Stderr, "encode result:", err)
		os.Exit(2)
	}
	if !comparison.Pass {
		os.Exit(1)
	}
}
