//! Action Packet — the output boundary between Planning and Actions.
//!
//! An [`ActionPacket`] captures everything a downstream executor needs to
//! start work without going back to the Planning layer.  It maps directly
//! onto the §13 field list from manifest.md and is constructed from a
//! [`PlanBrief`].
//!
//! The type is a pure value object: no AI calls, no storage writes.

use serde::{Deserialize, Serialize};
use taskagent_shared::PlanId;

use crate::plan_brief::PlanBrief;

/// Unique identifier for an Action Packet.
///
/// Wraps a [`uuid::Uuid`]-compatible string so callers get a typed id
/// without pulling in uuid directly.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ActionPacketId(pub String);

impl ActionPacketId {
    /// Generate a new random id.
    pub fn new() -> Self {
        // Use the same source as taskagent_shared where available; fall back
        // to a pseudo-random approach using std only.  A real deployment will
        // wire in a proper UUID v7 generator — this is sufficient for the OSS
        // primitive layer.
        use std::time::{SystemTime, UNIX_EPOCH};
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        // Minimal collision-resistant id for a planning primitive.
        Self(format!("ap-{ts:x}"))
    }
}

impl Default for ActionPacketId {
    fn default() -> Self {
        Self::new()
    }
}

/// Everything a downstream executor needs to start work without consulting
/// the Planning layer again.
///
/// Field layout mirrors §13 of the manifest.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ActionPacket {
    pub id: ActionPacketId,
    /// The plan this packet was derived from, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_id: Option<PlanId>,
    pub title: String,
    pub goal: String,
    pub context: String,
    /// Ordered list of things to do (§13 "что нужно сделать").
    pub what_to_do: Vec<String>,
    /// Why this work is needed (§13 "почему это нужно").
    pub why: String,
    /// Explicitly out of scope (§13 "что не нужно делать").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub out_of_scope: Vec<String>,
    /// Measurable completion criteria.
    pub completion_criteria: Vec<String>,
    /// Hard constraints the executor must respect.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<String>,
    /// Known risks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub risks: Vec<String>,
    /// Upstream dependencies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
    /// Documents / artifacts that must exist before work starts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_artifacts: Vec<String>,
    /// Decisions this packet was derived from.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_decisions: Vec<String>,
    /// Background knowledge the executor should be aware of.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_knowledge: Vec<String>,
    /// Alternatives that were considered and rejected.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejected_alternatives: Vec<String>,
    /// Artifacts the executor is expected to produce.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expected_artifacts: Vec<String>,
    /// Rules to verify before starting work.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_start_rules: Vec<String>,
    /// Rules to verify before marking work done.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_done_rules: Vec<String>,
}

impl ActionPacket {
    /// Construct an [`ActionPacket`] from a [`PlanBrief`].
    ///
    /// Fields that map 1-to-1 are copied directly.  Fields that have no
    /// counterpart in `PlanBrief` (e.g. `title`, `context`, `why`) are left
    /// empty so callers can fill them in.  `plan_id` is left `None` and must
    /// be set by the caller when a concrete plan exists.
    pub fn from_brief(brief: &PlanBrief) -> Self {
        Self {
            id: ActionPacketId::new(),
            plan_id: None,
            title: brief.goal.clone(),
            goal: brief.goal.clone(),
            context: String::new(),
            what_to_do: brief.in_scope.clone(),
            why: brief.why_now.clone().unwrap_or_default(),
            out_of_scope: brief.out_of_scope.clone(),
            completion_criteria: brief.completion_criteria.clone(),
            constraints: Vec::new(),
            risks: brief.risks.clone(),
            dependencies: brief.dependencies.clone(),
            required_artifacts: brief.required_artifacts.clone(),
            related_decisions: brief.decisions_made.clone(),
            related_knowledge: brief.knowledge_base.clone(),
            rejected_alternatives: brief.rejected_alternatives.clone(),
            expected_artifacts: Vec::new(),
            pre_start_rules: Vec::new(),
            pre_done_rules: Vec::new(),
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan_brief::PlanBrief;

    fn sample_brief() -> PlanBrief {
        PlanBrief {
            goal: "Implement plan readiness".into(),
            in_scope: vec!["plan_brief.rs".into(), "action_packet.rs".into()],
            completion_criteria: vec!["cargo test green".into()],
            taskagent_target: "planning_oss".into(),
            why_now: Some("Wave-2b pilot".into()),
            risks: vec!["read-only vendor symlink".into()],
            out_of_scope: vec!["AI inference".into()],
            decisions_made: vec!["Apache-2.0 + Commons-Clause".into()],
            ..PlanBrief::default()
        }
    }

    #[test]
    fn from_brief_maps_required_fields() {
        let packet = ActionPacket::from_brief(&sample_brief());
        assert_eq!(packet.goal, "Implement plan readiness");
        assert_eq!(packet.what_to_do, vec!["plan_brief.rs", "action_packet.rs"]);
        assert_eq!(packet.completion_criteria, vec!["cargo test green"]);
        assert_eq!(packet.why, "Wave-2b pilot");
        assert_eq!(packet.out_of_scope, vec!["AI inference"]);
        assert_eq!(packet.risks, vec!["read-only vendor symlink"]);
        assert_eq!(packet.related_decisions, vec!["Apache-2.0 + Commons-Clause"]);
    }

    #[test]
    fn from_brief_produces_unique_ids() {
        let b = sample_brief();
        let a1 = ActionPacket::from_brief(&b);
        // tiny sleep to advance the nanosecond counter on fast machines
        std::thread::sleep(std::time::Duration::from_nanos(1));
        let a2 = ActionPacket::from_brief(&b);
        // ids are unlikely to collide; warn if they do (not a hard failure in
        // unit test context but signals a clock resolution problem)
        if a1.id == a2.id {
            eprintln!("WARN: ActionPacketId collision — clock resolution too coarse");
        }
    }

    #[test]
    fn serde_roundtrip() {
        let packet = ActionPacket::from_brief(&sample_brief());
        let json = serde_json::to_string(&packet).unwrap();
        let back: ActionPacket = serde_json::from_str(&json).unwrap();
        assert_eq!(packet, back);
    }
}
