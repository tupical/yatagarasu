//! `yatagarasu` — the Planning layer skeleton.
//!
//! A self-contained open-core skeleton: it defines its own primitives,
//! domain output types, a prompt engine, and a provider-neutral
//! [`AiProvider`] seam. It has **no** dependency on daruma and **no**
//! dependency on sibling `*_oss` layers. the host supplies the concrete AI
//! provider and any daruma / decisions adapters when wiring the layer
//! into its architecture — implementations live only inside the host.
//!
//! The crate owns the AI **planning** operations — `decompose` (a task →
//! at least 2 sub-task drafts), `scope` (broaden/narrow a task by rewriting
//! its title + description) and `analyze_complexity` (batch-score a plan's
//! tasks for decomposition fan-out) — plus the deterministic plan readiness
//! brief ([`PlanBrief`] / [`check_readiness`]).
//!
//! # Contract
//! - Domain primitives stay storage-agnostic; the server persists plan briefs.
//! - All JSON is built with [`serde_json::json!`]; no string concatenation.
//! - Errors propagate as [`PlanningError`].

pub mod ai;
pub mod complexity;
pub mod decompose;
pub mod error;
pub mod plan;
pub mod plan_brief;
pub mod prompts;
pub mod scope;
pub mod task;
pub mod time;

// ── Seam re-exports ─────────────────────────────────────────────────────────────
pub use ai::{
    report_complexity_tool, rescope_task_tool, split_task_tool, wrap_untrusted, AiError, AiOutput,
    AiProvider, AiRequest, ToolCall,
};
pub use error::PlanningError;
pub use prompts::PromptRegistry;
pub use task::{Priority, ProjectId, Status, Task, TaskDraft, TaskId, TaskPatchDraft};
pub use time::Timestamp;

// ── Operation re-exports ────────────────────────────────────────────────────────
pub use complexity::{
    analyze_complexity_batch, build_analyze_complexity_prompt, ComplexityHintDraft, TaskBrief,
    MAX_BATCH_TASKS,
};
pub use decompose::{decompose_task, SplitDraft};
pub use plan::plan_ai;
pub use plan_brief::{check_readiness, PlanBrief, PlanReadinessReport};
pub use scope::{scope_task, ScopeDirection, UpdateDraft};

/// Start a plan brief with the decisions that it must preserve as lineage.
pub fn brief_from_decisions(decision_ids: &[String]) -> PlanBrief {
    PlanBrief {
        decisions_made: decision_ids.to_vec(),
        ..PlanBrief::default()
    }
}

#[cfg(test)]
mod adapter_tests {
    use super::*;

    #[test]
    fn decision_ids_are_preserved_in_brief() {
        let brief = brief_from_decisions(&["dec_1".into(), "dec_2".into()]);
        assert_eq!(brief.decisions_made, ["dec_1", "dec_2"]);
    }
}
