//! Batch task complexity analysis (§3.8.3).
//!
//! Given a slice of [`TaskBrief`] rows, issue **one** model call and return
//! a [`ComplexityHintDraft`] per task. The whole point of batching is to
//! amortise one prompt across N tasks rather than calling decompose N times.
//!
//! The planning layer owns the operation (prompt rendering, tool schema, arg
//! mapping) but not the model client: callers pass any [`AiProvider`]. The
//! output is provider-neutral and carries **no** persistence metadata — the
//! host assigns `batch_id` + `generated_at` and upserts the rows into
//! daruma's `task_complexity_hints` projection. The planning layer never
//! writes to storage.

use std::collections::HashMap;

use serde::Serialize;
use serde_json::Value;

use crate::ai::{report_complexity_tool, wrap_untrusted, AiOutput, AiProvider, AiRequest};
use crate::error::PlanningError;
use crate::prompts::PromptRegistry;
use crate::task::TaskId;

/// Hard cap on tasks per batch. Keeps prompt size predictable and the
/// model's per-task attention non-degenerate. Callers with more tasks
/// should chunk; we do not split for them here so the contract stays
/// "one model call per call".
pub const MAX_BATCH_TASKS: usize = 50;

/// Minimal task context handed to the analyser (title + optional
/// description). Provider-neutral mirror of daruma's `TaskBrief`, kept narrow
/// on purpose: the model only needs enough to gauge fan-out, and a smaller
/// payload keeps the batch prompt within token limits.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TaskBrief {
    pub task_id: TaskId,
    pub title: String,
    #[serde(default)]
    pub description: String,
}

/// The structured result of scoring one task, before it becomes a daruma
/// `ComplexityHint` row. Deliberately omits `batch_id` and `generated_at`:
/// those are persistence concerns the host assigns at write-back time, so
/// the planning layer stays free of clocks and storage identity.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct ComplexityHintDraft {
    pub task_id: TaskId,
    /// Decomposition complexity, clamped to 1..=10.
    pub score: u8,
    /// Suggested fan-out, clamped to 0..=20 (0 = no decomposition needed).
    pub recommended_subtasks: u8,
    /// One imperative sentence on how to break the task down.
    pub expansion_hint: String,
    /// Brief justification for the score.
    pub reasoning: String,
}

#[derive(Serialize)]
struct AnalyzeCtx<'a> {
    tasks_list: &'a str,
}

/// Build the batch-complexity prompt. Pure — kept separate so callers and
/// tests can inspect the exact string sent to the model without going
/// through a provider. The per-task list is rendered here (injection-hardened
/// as one untrusted block) and substituted into the `tasks_list` variable.
///
/// Panics only if the bundled `prompts/analyze_complexity.toml` is malformed
/// — a build-time invariant covered by `PromptRegistry`'s test suite.
pub fn build_analyze_complexity_prompt(tasks: &[TaskBrief]) -> String {
    let mut tasks_list = String::new();
    for (i, t) in tasks.iter().enumerate() {
        tasks_list.push_str(&format!("{}. [{}] {}\n", i + 1, t.task_id, t.title.trim()));
        if !t.description.is_empty() {
            // Indent so the model can see this is body, not a new task.
            for line in t.description.lines() {
                tasks_list.push_str("    ");
                tasks_list.push_str(line);
                tasks_list.push('\n');
            }
        }
    }
    let tasks_list = wrap_untrusted("task list", &tasks_list);
    PromptRegistry::load(
        "analyze_complexity",
        "default",
        &AnalyzeCtx {
            tasks_list: &tasks_list,
        },
    )
    .expect("bundled analyze_complexity prompt is well-formed (verified by PromptRegistry tests)")
}

/// Run one batch complexity analysis. Returns drafts in the order the model
/// emits them (it is asked for one row per input). Tasks the model omits are
/// simply absent in the returned vec; rows whose `task_id` is not one of the
/// inputs are dropped so the model cannot invent ids.
///
/// The concrete model client is supplied by the caller via [`AiProvider`].
pub async fn analyze_complexity_batch<P: AiProvider>(
    provider: &P,
    tasks: Vec<TaskBrief>,
) -> Result<Vec<ComplexityHintDraft>, PlanningError> {
    if tasks.is_empty() {
        return Ok(vec![]);
    }
    if tasks.len() > MAX_BATCH_TASKS {
        return Err(PlanningError::validation(format!(
            "analyze_complexity_batch: batch size {} exceeds MAX_BATCH_TASKS={}",
            tasks.len(),
            MAX_BATCH_TASKS
        )));
    }

    let prompt = build_analyze_complexity_prompt(&tasks);
    let req = AiRequest {
        input: Value::String(prompt),
        tools: vec![report_complexity_tool()],
        tool_choice: Some("required".into()),
    };

    let outputs = provider.respond(req).await?;
    let tc = outputs
        .into_iter()
        .find_map(|o| match o {
            AiOutput::ToolCall(tc) if tc.name == "report_complexity" => Some(tc),
            _ => None,
        })
        .ok_or_else(|| {
            PlanningError::ai("analyze_complexity_batch: model returned no report_complexity call")
        })?;

    let args: Value =
        serde_json::from_str(&tc.arguments).map_err(|e| PlanningError::serde(e.to_string()))?;
    let raw_hints = args["hints"]
        .as_array()
        .ok_or_else(|| PlanningError::validation("report_complexity: missing 'hints' array"))?;

    // Index inputs by id so we can validate the model's `task_id`s.
    let valid_ids: HashMap<String, TaskId> = tasks
        .iter()
        .map(|t| (t.task_id.to_string(), t.task_id))
        .collect();

    let mut out = Vec::with_capacity(raw_hints.len());
    for item in raw_hints {
        let Some(id_s) = item["task_id"].as_str() else {
            continue;
        };
        // Accept either prefixed (task_...) or bare UUID; resolve against the
        // input set so the model can't invent task ids.
        let task_id = match valid_ids.get(id_s) {
            Some(id) => *id,
            None => match id_s.parse::<TaskId>() {
                Ok(parsed) if valid_ids.values().any(|v| *v == parsed) => parsed,
                _ => continue,
            },
        };

        let score = item["score"].as_i64().unwrap_or(0).clamp(1, 10) as u8;
        let recommended_subtasks = item["recommended_subtasks"]
            .as_i64()
            .unwrap_or(0)
            .clamp(0, 20) as u8;
        let expansion_hint = item["expansion_hint"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_owned();
        let reasoning = item["reasoning"].as_str().unwrap_or("").trim().to_owned();

        out.push(ComplexityHintDraft {
            task_id,
            score,
            recommended_subtasks,
            expansion_hint,
            reasoning,
        });
    }

    Ok(out)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{AiError, ToolCall, UNTRUSTED_CLOSE, UNTRUSTED_OPEN};

    /// Minimal provider returning a fixed `report_complexity` call.
    struct FakeProvider {
        args: String,
    }

    impl AiProvider for FakeProvider {
        async fn respond(&self, _req: AiRequest) -> Result<Vec<AiOutput>, AiError> {
            Ok(vec![AiOutput::ToolCall(ToolCall {
                name: "report_complexity".into(),
                arguments: self.args.clone(),
            })])
        }
    }

    fn brief(title: &str, description: &str) -> TaskBrief {
        TaskBrief {
            task_id: TaskId::new(),
            title: title.into(),
            description: description.into(),
        }
    }

    #[test]
    fn prompt_lists_every_task_with_id_and_fences_list() {
        let a = brief("Wire DB layer", "");
        let b = brief("Add MCP tool", "two-line\nbody");
        let prompt = build_analyze_complexity_prompt(&[a.clone(), b.clone()]);
        assert!(prompt.contains(&a.task_id.to_string()));
        assert!(prompt.contains(&b.task_id.to_string()));
        assert!(prompt.contains("Wire DB layer"));
        assert!(prompt.contains("two-line"));
        assert!(prompt.contains(UNTRUSTED_OPEN));
        assert!(prompt.contains(UNTRUSTED_CLOSE));
    }

    #[test]
    fn max_batch_constant_is_reasonable() {
        // Stability guard: if we ever change this, callers need to know.
        const _: () = assert!(MAX_BATCH_TASKS >= 10 && MAX_BATCH_TASKS <= 200);
    }

    #[tokio::test]
    async fn empty_input_short_circuits() {
        let fake = FakeProvider { args: "{}".into() };
        let out = analyze_complexity_batch(&fake, vec![]).await.unwrap();
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn oversized_batch_is_rejected() {
        let fake = FakeProvider { args: "{}".into() };
        let tasks: Vec<TaskBrief> = (0..MAX_BATCH_TASKS + 1)
            .map(|i| brief(&format!("t{i}"), ""))
            .collect();
        let err = analyze_complexity_batch(&fake, tasks).await.unwrap_err();
        assert!(matches!(err, PlanningError::Validation(_)));
    }

    #[tokio::test]
    async fn maps_tool_call_to_drafts_and_clamps() {
        let a = brief("Wire DB layer", "");
        let b = brief("Add MCP tool", "");
        // a: out-of-range score/subtasks must clamp; b: in-range passthrough.
        let args = format!(
            r#"{{"hints":[
                {{"task_id":"{}","score":99,"recommended_subtasks":50,"expansion_hint":"  split DB ","reasoning":"big"}},
                {{"task_id":"{}","score":3,"recommended_subtasks":2,"expansion_hint":"add tool","reasoning":"small"}}
            ]}}"#,
            a.task_id, b.task_id
        );
        let fake = FakeProvider { args };
        let out = analyze_complexity_batch(&fake, vec![a.clone(), b.clone()])
            .await
            .unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].task_id, a.task_id);
        assert_eq!(out[0].score, 10); // clamped from 99
        assert_eq!(out[0].recommended_subtasks, 20); // clamped from 50
        assert_eq!(out[0].expansion_hint, "split DB"); // trimmed
        assert_eq!(out[1].task_id, b.task_id);
        assert_eq!(out[1].score, 3);
        assert_eq!(out[1].recommended_subtasks, 2);
    }

    #[tokio::test]
    async fn invented_task_ids_are_dropped() {
        let a = brief("Real task", "");
        let args = format!(
            r#"{{"hints":[
                {{"task_id":"{}","score":5,"recommended_subtasks":1,"expansion_hint":"x","reasoning":"y"}},
                {{"task_id":"task_00000000-0000-0000-0000-000000000000","score":7,"recommended_subtasks":3,"expansion_hint":"ghost","reasoning":"z"}}
            ]}}"#,
            a.task_id
        );
        let fake = FakeProvider { args };
        let out = analyze_complexity_batch(&fake, vec![a.clone()])
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].task_id, a.task_id);
    }

    #[tokio::test]
    async fn missing_tool_call_is_error() {
        struct TextOnly;
        impl AiProvider for TextOnly {
            async fn respond(&self, _req: AiRequest) -> Result<Vec<AiOutput>, AiError> {
                Ok(vec![AiOutput::Text("no tool here".into())])
            }
        }
        let err = analyze_complexity_batch(&TextOnly, vec![brief("t", "")])
            .await
            .unwrap_err();
        assert!(matches!(err, PlanningError::Ai(_)));
    }
}
