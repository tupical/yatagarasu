//! Plan readiness brief (§15) — deterministic structure and check.
//!
//! [`PlanBrief`] captures all 14 questions from manifest §15 that any plan
//! must answer before handing work off to Actions / Daruma.  It is a
//! pure data type: no AI calls, no storage writes.
//!
//! [`check_readiness`] inspects a brief and returns a [`PlanReadinessReport`]
//! that lists which fields are filled (`allowed`) and which are missing
//! (`missing`).  The plan is considered ready when `missing` is empty.
//!
//! ## Required vs recommended fields
//!
//! **Required** (absence blocks readiness):
//! `goal`, `in_scope`, `completion_criteria`, `daruma_target`
//!
//! **Recommended** (absence is noted but does not block):
//! `why_now`, `decisions_made`, `risks`

use serde::{Deserialize, Serialize};

// ── Types ─────────────────────────────────────────────────────────────────────

/// All 14 questions from manifest §15 that a plan must answer.
///
/// String fields that are optional but meaningful use `Option<String>`.
/// List fields default to empty `Vec` — an empty vec is treated the same
/// as `None` by [`check_readiness`] for its "filled?" test.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanBrief {
    // ── Required ──────────────────────────────────────────────────────────────
    /// §15 Q1: Which goal does the plan advance?
    pub goal: String,
    /// §15 Q7: What is in scope?
    pub in_scope: Vec<String>,
    /// §15 Q12: What are the completion criteria?
    pub completion_criteria: Vec<String>,
    /// §15 Q13: What should land in Daruma?
    pub daruma_target: String,

    // ── Recommended ───────────────────────────────────────────────────────────
    /// §15 Q2: Why is this plan needed now?
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why_now: Option<String>,
    /// §15 Q3: Which decisions have already been made?
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decisions_made: Vec<String>,
    /// §15 Q9: What risks are known?
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub risks: Vec<String>,
    /// Hard boundaries the plan must respect. Not one of the 14 §15
    /// questions, but a first-class field so a constraint set upstream is
    /// not lost on the way to Actions. mcpbox fills this from the Decisions
    /// layer's `constraint` directives when wiring the pipeline.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<String>,

    // ── Optional ──────────────────────────────────────────────────────────────
    /// §15 Q4: What knowledge underpins the plan?
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub knowledge_base: Vec<String>,
    /// §15 Q5: Which hypotheses remain unverified?
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unverified_hypotheses: Vec<String>,
    /// §15 Q6: Which alternatives were rejected and why?
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejected_alternatives: Vec<String>,
    /// §15 Q8: What is explicitly out of scope?
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub out_of_scope: Vec<String>,
    /// §15 Q10: What dependencies exist?
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
    /// §15 Q11: Which documents or artifacts are mandatory?
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_artifacts: Vec<String>,
    /// §15 Q14: One project, many projects, or no new project?
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_structure: Option<String>,
}

/// Result of [`check_readiness`].
///
/// `is_ready` is `true` iff `missing` is empty (all required fields filled).
/// `missing` lists required fields that are absent/empty.
/// `allowed` lists all fields that are filled (required or recommended).
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanReadinessReport {
    pub is_ready: bool,
    pub missing: Vec<String>,
    pub allowed: Vec<String>,
}

// ── Logic ─────────────────────────────────────────────────────────────────────

/// Check whether `brief` satisfies the §15 readiness contract.
///
/// Returns a [`PlanReadinessReport`] with `is_ready = true` when all
/// required fields are filled; lists which fields are present (`allowed`)
/// and which are absent (`missing`).  The check is purely deterministic —
/// no network calls, no side effects.
pub fn check_readiness(brief: &PlanBrief) -> PlanReadinessReport {
    let mut missing: Vec<String> = Vec::new();
    let mut allowed: Vec<String> = Vec::new();

    // ── Required fields ───────────────────────────────────────────────────────
    check_str(brief.goal.as_str(), "goal", &mut missing, &mut allowed);
    check_vec(&brief.in_scope, "in_scope", &mut missing, &mut allowed);
    check_vec(
        &brief.completion_criteria,
        "completion_criteria",
        &mut missing,
        &mut allowed,
    );
    check_str(
        brief.daruma_target.as_str(),
        "daruma_target",
        &mut missing,
        &mut allowed,
    );

    // ── Recommended fields (noted but do not block) ───────────────────────────
    check_opt(brief.why_now.as_deref(), "why_now", &mut allowed);
    check_vec_opt(&brief.decisions_made, "decisions_made", &mut allowed);
    check_vec_opt(&brief.risks, "risks", &mut allowed);

    PlanReadinessReport {
        is_ready: missing.is_empty(),
        missing,
        allowed,
    }
}

// ── Helpers (private) ─────────────────────────────────────────────────────────

fn check_str(
    value: &str,
    name: &'static str,
    missing: &mut Vec<String>,
    allowed: &mut Vec<String>,
) {
    if value.trim().is_empty() {
        missing.push(name.to_owned());
    } else {
        allowed.push(name.to_owned());
    }
}

fn check_vec(
    value: &[String],
    name: &'static str,
    missing: &mut Vec<String>,
    allowed: &mut Vec<String>,
) {
    if value.is_empty() {
        missing.push(name.to_owned());
    } else {
        allowed.push(name.to_owned());
    }
}

/// Recommended-only: contributes to `allowed` when present, silently absent.
fn check_opt(value: Option<&str>, name: &'static str, allowed: &mut Vec<String>) {
    if value.map(str::trim).filter(|s| !s.is_empty()).is_some() {
        allowed.push(name.to_owned());
    }
}

fn check_vec_opt(value: &[String], name: &'static str, allowed: &mut Vec<String>) {
    if !value.is_empty() {
        allowed.push(name.to_owned());
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn full_brief() -> PlanBrief {
        PlanBrief {
            goal: "Ship Plan-readiness primitive".into(),
            in_scope: vec!["plan_brief.rs".into()],
            completion_criteria: vec!["cargo test green".into()],
            daruma_target: "yatagarasu project".into(),
            why_now: Some("Wave-2b pilot needs it first".into()),
            decisions_made: vec!["Apache-2.0 + Commons-Clause".into()],
            risks: vec!["vendor/oss symlink is read-only".into()],
            ..PlanBrief::default()
        }
    }

    #[test]
    fn ready_when_all_required_filled() {
        let report = check_readiness(&full_brief());
        assert!(report.is_ready);
        assert!(report.missing.is_empty());
        // required fields appear in allowed
        assert!(report.allowed.iter().any(|f| f == "goal"));
        assert!(report.allowed.iter().any(|f| f == "in_scope"));
        assert!(report.allowed.iter().any(|f| f == "completion_criteria"));
        assert!(report.allowed.iter().any(|f| f == "daruma_target"));
        // recommended fields also surfaced
        assert!(report.allowed.iter().any(|f| f == "why_now"));
        assert!(report.allowed.iter().any(|f| f == "decisions_made"));
        assert!(report.allowed.iter().any(|f| f == "risks"));
    }

    #[test]
    fn not_ready_when_required_fields_missing() {
        let brief = PlanBrief::default(); // everything empty
        let report = check_readiness(&brief);
        assert!(!report.is_ready);
        assert!(report.missing.iter().any(|f| f == "goal"));
        assert!(report.missing.iter().any(|f| f == "in_scope"));
        assert!(report.missing.iter().any(|f| f == "completion_criteria"));
        assert!(report.missing.iter().any(|f| f == "daruma_target"));
        assert!(report.allowed.is_empty());
    }

    #[test]
    fn partial_brief_lists_only_missing_required() {
        let brief = PlanBrief {
            goal: "Some goal".into(),
            in_scope: vec!["src/".into()],
            // completion_criteria and daruma_target missing
            ..PlanBrief::default()
        };
        let report = check_readiness(&brief);
        assert!(!report.is_ready);
        assert!(report.missing.iter().any(|f| f == "completion_criteria"));
        assert!(report.missing.iter().any(|f| f == "daruma_target"));
        assert!(!report.missing.iter().any(|f| f == "goal"));
        assert!(!report.missing.iter().any(|f| f == "in_scope"));
    }

    #[test]
    fn recommended_fields_do_not_block_readiness() {
        // Only required fields filled; recommended absent.
        let brief = PlanBrief {
            goal: "goal".into(),
            in_scope: vec!["scope".into()],
            completion_criteria: vec!["done".into()],
            daruma_target: "target".into(),
            ..PlanBrief::default()
        };
        let report = check_readiness(&brief);
        assert!(report.is_ready, "missing recommended fields must not block");
        assert!(report.missing.is_empty());
    }

    #[test]
    fn serde_roundtrip() {
        let brief = full_brief();
        let json = serde_json::to_string(&brief).unwrap();
        let back: PlanBrief = serde_json::from_str(&json).unwrap();
        assert_eq!(brief, back);
    }
}
