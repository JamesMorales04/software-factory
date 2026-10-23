// s4d-baseline emits the frozen deterministic S4D-RULES-1 outputs for the
// corpus. It never calls a model.
package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"os"

	"github.com/JamesMorales04/software-factory/core/internal/lead"
)

func main() {
	corpusPath := flag.String("corpus", "core/internal/lead/testdata/s4d-evaluation-corpus.json", "frozen corpus")
	outPath := flag.String("out", "", "output JSON")
	flag.Parse()
	raw, err := os.ReadFile(*corpusPath)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(2)
	}
	var corpus lead.EvaluationCorpus
	if err := json.Unmarshal(raw, &corpus); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(2)
	}
	output := map[string]any{"baselineId": lead.S4DBaselineID, "fixtures": map[string]any{}}
	fixtures := output["fixtures"].(map[string]any)
	for _, fixture := range corpus.Fixtures {
		fixtures[fixture.ID] = lead.EvaluateS4DBaseline(lead.BaselineInput{
			Request: fixture.Request, Evidence: fixture.Evidence,
		})
	}
	body, _ := json.MarshalIndent(output, "", "  ")
	body = append(body, '\n')
	if *outPath == "" {
		_, _ = os.Stdout.Write(body)
		return
	}
	if err := os.WriteFile(*outPath, body, 0o600); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(2)
	}
}
