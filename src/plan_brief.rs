//! Plan readiness brief (§15) — deterministic structure and check.
//!
//! [`PlanBrief`] captures all 14 questions from manifest §15 that any plan
//! must answer before handing work off to Actions / TaskAgent.  It is a
//! pure data type: no AI calls, no storage writes.
//!
//! [`check_readiness`] inspects a brief and returns a [`PlanReadinessReport`]
//! that lists which fields are filled (`allowed`) and which are missing
//! (`missing`).  The plan is considered ready when `missing` is empty.
//!
//! ## Required vs recommended fields
//!
//! **Required** (absence blocks readiness):
//! `goal`, `in_scope`, `completion_criteria`, `taskagent_target`
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
    /// §15 Q13: What should land in TaskAgent?
    pub taskagent_target: String,

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
    /// Hard boundaries the plan must respect, carried over from the
    /// Decisions layer's `constraint` directives. Not one of the 14 §15
    /// questions, but a first-class field so a constraint set upstream is
    /// not lost on the way to Actions.
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

// ── Decisions → Planning adapter ────────────────────────────────────────────────

impl PlanBrief {
    /// Build a [`PlanBrief`] from the upstream Decisions layer, preserving
    /// the lineage of every choice it rests on.
    ///
    /// Planning sits downstream of Decisions in the MCPBox pipeline
    /// (Intake → Sensemaking → Decisions → **Planning** → Actions;
    /// vision.md §6.3): a plan is the *realisation* of direction that has
    /// already been fixed. This adapter turns that fixed direction into a
    /// draft brief.
    ///
    /// `directives` drive the §15 frame:
    /// - the first [`Goal`](decisions_oss::DirectiveKind::Goal) directive
    ///   becomes the brief's `goal`;
    /// - every
    ///   [`SuccessCriteria`](decisions_oss::DirectiveKind::SuccessCriteria)
    ///   becomes a `completion_criteria` entry;
    /// - every
    ///   [`Constraint`](decisions_oss::DirectiveKind::Constraint) becomes a
    ///   `constraints` entry;
    /// - every [`NonGoal`](decisions_oss::DirectiveKind::NonGoal) becomes an
    ///   `out_of_scope` entry.
    ///
    /// `decisions` populate `decisions_made` — and this is the load-bearing
    /// part: each entry is the source [`Decision`](decisions_oss::Decision)'s
    /// id (its `Display` form, e.g. `dec_<uuid>`), so the produced brief
    /// always knows *which* recorded choices it descends from. Lineage is
    /// the core value MCPBox preserves; a plan that forgot its decisions
    /// could not be audited later.
    ///
    /// The brief that comes back is a *draft*: fields with no Decisions-layer
    /// counterpart (`in_scope`, `taskagent_target`, …) are left empty for the
    /// planner to fill, so a freshly-adapted brief will usually not yet pass
    /// [`check_readiness`].
    pub fn from_decisions(
        decisions: &[decisions_oss::Decision],
        directives: &[decisions_oss::Directive],
    ) -> Self {
        use decisions_oss::DirectiveKind;

        let goal = directives
            .iter()
            .find(|d| d.kind == DirectiveKind::Goal)
            .map(|d| d.statement.clone())
            .unwrap_or_default();

        let collect = |kind: DirectiveKind| -> Vec<String> {
            directives
                .iter()
                .filter(|d| d.kind == kind)
                .map(|d| d.statement.clone())
                .collect()
        };

        Self {
            goal,
            completion_criteria: collect(DirectiveKind::SuccessCriteria),
            constraints: collect(DirectiveKind::Constraint),
            out_of_scope: collect(DirectiveKind::NonGoal),
            // Lineage: keep the id of every source decision so the plan can
            // be traced back to the choices it realises.
            decisions_made: decisions.iter().map(|d| d.id.to_string()).collect(),
            ..Self::default()
        }
    }
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
        brief.taskagent_target.as_str(),
        "taskagent_target",
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
            taskagent_target: "planning_oss project".into(),
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
        assert!(report.allowed.iter().any(|f| f == "taskagent_target"));
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
        assert!(report.missing.iter().any(|f| f == "taskagent_target"));
        assert!(report.allowed.is_empty());
    }

    #[test]
    fn partial_brief_lists_only_missing_required() {
        let brief = PlanBrief {
            goal: "Some goal".into(),
            in_scope: vec!["src/".into()],
            // completion_criteria and taskagent_target missing
            ..PlanBrief::default()
        };
        let report = check_readiness(&brief);
        assert!(!report.is_ready);
        assert!(report.missing.iter().any(|f| f == "completion_criteria"));
        assert!(report.missing.iter().any(|f| f == "taskagent_target"));
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
            taskagent_target: "target".into(),
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

    // ── Decisions → Planning adapter ─────────────────────────────────────────

    use decisions_oss::{
        Actor, Decision, Directive, DirectiveKind, NewDecision, NewDirective,
    };

    fn decision(statement: &str) -> Decision {
        NewDecision {
            id: None,
            statement: statement.into(),
            decided_by: Actor::user(),
            decided_at: None,
            rationale: String::new(),
            alternatives: vec![],
            consequences: vec![],
            revisit_when: String::new(),
            links: vec![],
        }
        .into_decision(decisions_oss::time::now())
        .expect("valid decision")
    }

    fn directive(kind: DirectiveKind, statement: &str) -> Directive {
        NewDirective {
            id: None,
            kind,
            statement: statement.into(),
            set_by: Actor::user(),
            rationale: String::new(),
            links: vec![],
        }
        .into_directive(decisions_oss::time::now())
        .expect("valid directive")
    }

    #[test]
    fn from_decisions_maps_directive_kinds() {
        let directives = vec![
            directive(DirectiveKind::Goal, "Ship the planning adapter"),
            directive(DirectiveKind::SuccessCriteria, "cargo test green"),
            directive(DirectiveKind::Constraint, "no actions_oss dependency"),
            directive(DirectiveKind::NonGoal, "no AI inference here"),
        ];
        let brief = PlanBrief::from_decisions(&[], &directives);

        assert_eq!(brief.goal, "Ship the planning adapter");
        assert_eq!(brief.completion_criteria, vec!["cargo test green"]);
        assert_eq!(brief.constraints, vec!["no actions_oss dependency"]);
        assert_eq!(brief.out_of_scope, vec!["no AI inference here"]);
    }

    #[test]
    fn from_decisions_preserves_source_decision_ids() {
        let decisions = vec![
            decision("Use PlanBrief as the planning output"),
            decision("Keep ActionPacket in actions_oss only"),
        ];
        let expected: Vec<String> = decisions.iter().map(|d| d.id.to_string()).collect();

        let brief = PlanBrief::from_decisions(&decisions, &[]);

        // Lineage is the point: every source decision's id survives, in order.
        assert_eq!(brief.decisions_made, expected);
        assert!(brief.decisions_made.iter().all(|id| id.starts_with("dec_")));
    }

    #[test]
    fn from_decisions_produces_a_draft_not_yet_ready() {
        // With only a goal, the brief is missing required §15 fields and so
        // is a draft for the planner to finish — not yet ready.
        let directives = vec![directive(DirectiveKind::Goal, "some goal")];
        let brief = PlanBrief::from_decisions(&[], &directives);
        let report = check_readiness(&brief);
        assert!(!report.is_ready);
        assert!(report.missing.iter().any(|f| f == "in_scope"));
        assert!(report.missing.iter().any(|f| f == "taskagent_target"));
    }
}
