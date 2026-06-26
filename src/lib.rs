//! `yatagarasu` — the Planning layer skeleton.
//!
//! A self-contained open-core skeleton: it defines its own primitives,
//! domain output types, a prompt engine, and a provider-neutral
//! [`AiProvider`] seam. It has **no** dependency on daruma and **no**
//! dependency on sibling `*_oss` layers. mcpbox supplies the concrete AI
//! provider and any daruma / decisions adapters when wiring the layer
//! into its architecture — implementations live only inside mcpbox.
//!
//! The crate owns the AI **planning** operations — `decompose` (a task →
//! at least 2 sub-task drafts) and `scope` (broaden/narrow a task by
//! rewriting its title + description) — plus the deterministic plan
//! readiness brief ([`PlanBrief`] / [`check_readiness`]).
//!
//! # Contract
//! - The planning layer never writes to storage. `decompose` returns a
//!   [`SplitDraft`] and `scope` returns an [`UpdateDraft`]; the caller
//!   (mcpbox) maps them onto daruma `Command`s and dispatches.
//! - All JSON is built with [`serde_json::json!`]; no string concatenation.
//! - Errors propagate as [`PlanningError`].

pub mod ai;
pub mod decompose;
pub mod error;
pub mod plan_brief;
pub mod prompts;
pub mod scope;
pub mod task;
pub mod time;

// ── Seam re-exports ─────────────────────────────────────────────────────────────
pub use ai::{
    rescope_task_tool, split_task_tool, wrap_untrusted, AiError, AiOutput, AiProvider, AiRequest,
    ToolCall,
};
pub use error::PlanningError;
pub use prompts::PromptRegistry;
pub use task::{Priority, ProjectId, Status, Task, TaskDraft, TaskId, TaskPatchDraft};
pub use time::Timestamp;

// ── Operation re-exports ────────────────────────────────────────────────────────
pub use decompose::{decompose_task, SplitDraft};
pub use plan_brief::{check_readiness, PlanBrief, PlanReadinessReport};
pub use scope::{scope_task, ScopeDirection, UpdateDraft};
