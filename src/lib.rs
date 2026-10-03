//! `yatagarasu` — the Planning layer skeleton.
//!
//! A self-contained open-core skeleton: it defines its own primitives,
//! domain output types, a prompt engine, and a provider-neutral
//! [`AiProvider`] seam. It has **no** dependency on daruma and **no**
//! dependency on sibling `*_oss` layers. the host supplies the concrete AI
//! provider and any daruma / decisions adapters when wiring the layer
//! into its architecture — implementations live only inside the host.
//!
//! The crate implements [`plan_ai`] (decision context → [`PlanBrief`]),
//! `decompose` (task → sub-task drafts), `scope` (rewrite a task's scope),
//! and [`check_readiness`]. Batch complexity scoring is implemented in Daruma.
//!
//! # Contract
//! - Domain primitives stay storage-agnostic; the server persists plan briefs.
//! - All JSON is built with [`serde_json::json!`]; no string concatenation.
//! - Errors propagate as [`PlanningError`].

pub mod ai;
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
    rescope_task_tool, split_task_tool, wrap_untrusted, AiError, AiOutput, AiProvider, AiRequest,
    AiUsage, ToolCall,
};
pub use error::PlanningError;
pub use prompts::PromptRegistry;
pub use task::{Priority, ProjectId, Status, Task, TaskDraft, TaskId, TaskPatchDraft};
pub use time::Timestamp;

// ── Operation re-exports ────────────────────────────────────────────────────────
pub use decompose::{decompose_task, SplitDraft};
pub use plan::{plan_ai, PlanError};
pub use plan_brief::{check_readiness, PlanBrief, PlanReadinessReport};
pub use scope::{scope_task, ScopeDirection, UpdateDraft};
