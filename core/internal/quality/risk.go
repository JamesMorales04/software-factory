// Package quality owns the deterministic Core quality policy: final-diff
// risk classification and the Quality Gate evaluation. It never calls a
// model and never accepts evidence on trust; inputs are durable records.
package quality

import "strings"

// Protected path classes. Authorization, policy, persistence, security and
// delivery surfaces cannot be accepted by the ordinary R0/R1 path.
var criticalPathPrefixes = []string{
	"core/internal/domain/",
	"core/internal/quality/",
	"core/internal/store/",
	"core/internal/api/",
	"core/internal/candidate/",
	"core/internal/exec/",
	"core/internal/paseo/",
	"core/internal/telemetry/",
	"core/cmd/factory/",
	"core/cmd/factory-core/",
	"core/cmd/factory-worker/",
	"core/cmd/factory-verifier/",
	"core/cmd/factory-reviewer/",
	"scripts/",
	".github/",
	"AGENTS.md",
	"docs/security/",
	"docs/quality/",
	"docs/architecture/decisions/",
}

var contractPathPrefixes = []string{
	"docs/contracts/",
	"docs/architecture/",
	"core/go.mod",
	"core/go.sum",
	"office/package.json",
	"office/package-lock.json",
	"core/internal/store/sqlite.go",
}

var credentialPathPatterns = []string{
	".env", "credentials", "secrets", ".npmrc", ".netrc", "id_rsa", "id_ed25519",
}

// ChangeRiskResult is the deterministic risk classification of the actual
// final diff. Reasons are bounded, human-readable facts.
type ChangeRiskResult struct {
	RiskClass string
	Reasons   []string
}

// ClassifyChange computes the risk of the exact changed paths. The result
// can only raise risk above R0; it is never lowered by a spec label.
func ClassifyChange(paths []string) ChangeRiskResult {
	result := ChangeRiskResult{RiskClass: "R0"}
	for _, changed := range paths {
		lower := strings.ToLower(changed)
		if isCredentialPath(lower) {
			result.raise("R3", "diff touches a credential-named path: "+changed)
			continue
		}
		if matchAnyPrefix(changed, criticalPathPrefixes) {
			result.raise("R3", "diff touches a protected authority/policy/security path: "+changed)
			continue
		}
		if matchAnyPrefix(changed, contractPathPrefixes) {
			result.raise("R2", "diff touches a contract, schema or dependency path: "+changed)
			continue
		}
		if strings.HasPrefix(changed, "core/") || strings.HasPrefix(changed, "office/") {
			result.raise("R1", "diff touches product source outside the protected set: "+changed)
		}
	}
	return result
}

func (r *ChangeRiskResult) raise(class, reason string) {
	if riskRank(class) > riskRank(r.RiskClass) {
		r.RiskClass = class
	}
	if len(r.Reasons) < 8 {
		r.Reasons = append(r.Reasons, reason)
	}
}

func riskRank(class string) int {
	switch class {
	case "R3":
		return 3
	case "R2":
		return 2
	case "R1":
		return 1
	default:
		return 0
	}
}

func matchAnyPrefix(value string, prefixes []string) bool {
	for _, prefix := range prefixes {
		if strings.HasSuffix(prefix, "/") {
			if strings.HasPrefix(value, prefix) {
				return true
			}
			continue
		}
		if value == prefix || strings.HasPrefix(value, prefix+"/") {
			return true
		}
	}
	return false
}

func isCredentialPath(lower string) bool {
	base := lower
	if idx := strings.LastIndex(lower, "/"); idx >= 0 {
		base = lower[idx+1:]
	}
	for _, pattern := range credentialPathPatterns {
		if base == pattern || strings.HasPrefix(base, pattern+".") || strings.Contains(base, pattern) {
			return true
		}
	}
	return false
}
