//! `planning-oss` — the Planning layer extracted from the TaskAgent OSS core.
//!
//! This crate owns the AI **planning** operations — `decompose` (a task →
//! `Command::SplitTask` with 2–6 sub-tasks) and `scope` (broaden/narrow a
//! task by rewriting its title + description) — on top of the
//! provider-neutral [`taskagent_ai_infra`] infrastructure (Responses API
//! client, [`AiProvider`] abstraction, prompt rendering engine, tool
//! schemas, prompt-injection hardening).
//!
//! It is the Wave-2b pilot for cross-repo operation extraction: the
//! operations and their prompts moved out of `taskagent/crates/ai` into
//! this separate repository, while the shared infrastructure is consumed
//! read-only via the `vendor/oss` symlink (mirroring the `mcpbox.ru`
//! vendoring pattern).
//!
//! # Contract (inherited from the core AI layer)
//! - The planning layer **never** writes to storage. Every output is a
//!   [`taskagent_core::Command`].
//! - All JSON is built with [`serde_json::json!`]; no string concatenation.
//! - Errors propagate as [`taskagent_shared::CoreError`].

pub mod decompose;
pub mod prompts;
pub mod scope;

// ── Re-export the infrastructure layer ─────────────────────────────────────────
//
// Preserves the operation crate's public surface (`OpenAiClient`,
// `AiConfig`, `AiProvider`, …) so callers depend on `planning_oss::*`
// without also naming `taskagent_ai_infra`.
pub use taskagent_ai_infra::{wrap_untrusted, AiConfig, AiError, AiProvider, OpenAiClient};

// The planning prompt catalogue, owned by this crate.
pub use prompts::PromptRegistry;

// ── Operation re-exports ────────────────────────────────────────────────────────

pub use decompose::decompose_task;
pub use scope::{scope_task, ScopeDirection};
