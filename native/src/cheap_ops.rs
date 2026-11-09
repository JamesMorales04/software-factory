//! Cheap Ops: deterministic interpretation of test/log receipt evidence.
//!
//! This is deliberately not an authority. It summarizes what receipts and the
//! Gate already say, labels itself OBSERVATION_ONLY and cannot accept, repair,
//! widen scope or replace the Quality Gate. No model call is required.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptObservation {
    pub role: String,
    pub principal_id: String,
    pub result: String,
    pub evidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheapOpsReport {
    pub authority: String,
    pub summary: String,
    pub passed: u32,
    pub failed: u32,
    pub not_tested: u32,
    pub reviewer_count: u32,
    pub verifier_present: bool,
    pub gate_outcome: Option<String>,
    pub gate_reasons: Vec<String>,
    pub notes: Vec<String>,
}

/// Interpret receipts without making a pass/fail decision. The Gate outcome is
/// copied verbatim as context and is never recomputed here.
pub fn interpret_receipts(
    receipts: &[ReceiptObservation],
    gate_outcome: Option<&str>,
    gate_reasons: &[String],
) -> CheapOpsReport {
    let mut passed = 0;
    let mut failed = 0;
    let mut not_tested = 0;
    let mut notes = Vec::new();
    let mut verifier_present = false;
    let mut reviewer_count = 0;
    for receipt in receipts {
        match receipt.role.as_str() {
            "verifier" => verifier_present = true,
            "reviewer" => reviewer_count += 1,
            _ => notes.push(format!("unrecognized receipt role {}", receipt.role)),
        }
        match receipt.result.as_str() {
            "PASS" => passed += 1,
            "FAIL" | "CHANGES_REQUESTED" => failed += 1,
            "NOT_TESTED" => not_tested += 1,
            other => notes.push(format!(
                "receipt {} has unrecognized result {other}",
                receipt.principal_id
            )),
        }
        if receipt.evidence == "MOCKED" {
            notes.push(format!(
                "receipt {} used mocked evidence; it cannot satisfy the Gate",
                receipt.principal_id
            ));
        } else if receipt.evidence == "UNKNOWN" {
            notes.push(format!(
                "receipt {} has unknown evidence provenance",
                receipt.principal_id
            ));
        }
    }
    if receipts.is_empty() {
        notes.push("no receipt evidence is recorded".into());
    }
    if reviewer_count > 1 {
        notes.push(format!(
            "{reviewer_count} independent reviewer receipts are aggregated by the Gate"
        ));
    }
    let summary = format!(
        "{} receipt(s): {} pass, {} fail/change, {} not tested; {} reviewer(s); verifier {}.",
        receipts.len(),
        passed,
        failed,
        not_tested,
        reviewer_count,
        if verifier_present {
            "recorded"
        } else {
            "missing"
        }
    );
    CheapOpsReport {
        authority: "OBSERVATION_ONLY".into(),
        summary,
        passed,
        failed,
        not_tested,
        reviewer_count,
        verifier_present,
        gate_outcome: gate_outcome.map(str::to_owned),
        gate_reasons: gate_reasons.to_vec(),
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(role: &str, result: &str, evidence: &str) -> ReceiptObservation {
        ReceiptObservation {
            role: role.into(),
            principal_id: format!("{role}-1"),
            result: result.into(),
            evidence: evidence.into(),
        }
    }

    #[test]
    fn summary_is_observation_only_and_copies_the_gate() {
        let report = interpret_receipts(
            &[
                receipt("verifier", "PASS", "EXECUTED"),
                receipt("reviewer", "PASS", "EXECUTED"),
                receipt("reviewer", "PASS", "EXECUTED"),
            ],
            Some("ACCEPTED"),
            &[],
        );
        assert_eq!(report.authority, "OBSERVATION_ONLY");
        assert_eq!(report.reviewer_count, 2);
        assert_eq!(report.gate_outcome.as_deref(), Some("ACCEPTED"));
        assert!(report.summary.contains("2 reviewer(s)"));
        assert!(report.notes.iter().any(|note| note.contains("aggregated")));
    }

    #[test]
    fn mocked_or_unknown_evidence_is_called_out_without_overriding_gate() {
        let report = interpret_receipts(
            &[
                receipt("verifier", "PASS", "MOCKED"),
                receipt("reviewer", "PASS", "UNKNOWN"),
            ],
            Some("NOT_TESTED"),
            &["receipt evidence provenance is unknown".into()],
        );
        assert_eq!(report.gate_outcome.as_deref(), Some("NOT_TESTED"));
        assert!(report.notes.iter().any(|note| note.contains("mocked")));
        assert!(report.notes.iter().any(|note| note.contains("unknown")));
    }
}
