//! Task scope adjustment — broaden or narrow an existing task.
//!
//! The model rewrites the task's title + description at a target
//! complexity level (`up` = broader / epic-style, `down` = narrower /
//! one concrete action). The output is a provider-neutral [`UpdateDraft`];
//! mcpbox turns it into taskagent's `Command::UpdateTask { id, patch }`.

use serde::Serialize;
use serde_json::Value;

use crate::ai::{rescope_task_tool, wrap_untrusted, AiOutput, AiProvider, AiRequest};
use crate::error::PlanningError;
use crate::prompts::PromptRegistry;
use crate::task::{Task, TaskId, TaskPatchDraft};

/// Direction the rewrite should move in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeDirection {
    Up,
    Down,
}

impl ScopeDirection {
    pub fn as_variant(self) -> &'static str {
        match self {
            ScopeDirection::Up => "up",
            ScopeDirection::Down => "down",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, PlanningError> {
        match raw {
            "up" | "broaden" => Ok(ScopeDirection::Up),
            "down" | "narrow" => Ok(ScopeDirection::Down),
            other => Err(PlanningError::validation(format!(
                "unknown scope direction: {other} (expected 'up' or 'down')"
            ))),
        }
    }
}

/// The structured result of the `scope` operation: a sparse update for a
/// specific task, before it becomes a taskagent `Command::UpdateTask`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct UpdateDraft {
    pub id: TaskId,
    pub patch: TaskPatchDraft,
}

#[derive(Serialize)]
struct ScopeCtx<'a> {
    title: &'a str,
    description: &'a str,
}

/// Build the scope prompt. Pure — exposed for tests.
pub fn build_scope_prompt(task: &Task, direction: ScopeDirection) -> String {
    let title = wrap_untrusted("task title", &task.title);
    let description = wrap_untrusted("task description", &task.description);
    PromptRegistry::load(
        "scope",
        direction.as_variant(),
        &ScopeCtx {
            title: &title,
            description: &description,
        },
    )
    .expect("bundled scope prompt is well-formed")
}

/// Ask the model to rescope `task`, returning an [`UpdateDraft`] with the
/// rewritten title + description. The concrete model client is supplied by
/// the caller via [`AiProvider`].
pub async fn scope_task<P: AiProvider>(
    provider: &P,
    task: &Task,
    direction: ScopeDirection,
) -> Result<UpdateDraft, PlanningError> {
    let prompt = build_scope_prompt(task, direction);

    let req = AiRequest {
        input: Value::String(prompt),
        tools: vec![rescope_task_tool()],
        tool_choice: Some("required".into()),
    };

    let outputs = provider.respond(req).await?;

    let tc = outputs
        .into_iter()
        .find_map(|o| match o {
            AiOutput::ToolCall(tc) if tc.name == "rescope_task" => Some(tc),
            _ => None,
        })
        .ok_or_else(|| PlanningError::ai("scope_task: model returned no rescope_task call"))?;

    let args: Value =
        serde_json::from_str(&tc.arguments).map_err(|e| PlanningError::serde(e.to_string()))?;

    let title = args["title"]
        .as_str()
        .ok_or_else(|| PlanningError::ai("rescope_task: missing 'title' in tool args"))?
        .trim()
        .to_owned();
    if title.is_empty() {
        return Err(PlanningError::ai("rescope_task: empty title"));
    }
    let description = args["description"].as_str().unwrap_or("").to_owned();

    let patch = TaskPatchDraft {
        title: Some(title),
        description: Some(description),
    };
    Ok(UpdateDraft { id: task.id, patch })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{AiError, ToolCall};
    use crate::task::{Priority, ProjectId, Status};
    use crate::time;

    fn sample_task() -> Task {
        let now = time::now();
        Task {
            id: TaskId::new(),
            project_id: Some(ProjectId::new()),
            title: "Wire login form".into(),
            description: "Connect to /v1/auth/login and store the bearer token.".into(),
            status: Status::Todo,
            priority: Priority::P2,
            created_at: now,
            updated_at: now,
        }
    }

    /// Minimal provider returning a fixed `rescope_task` call.
    struct FakeProvider {
        args: String,
    }

    impl AiProvider for FakeProvider {
        async fn respond(&self, _req: AiRequest) -> Result<Vec<AiOutput>, AiError> {
            Ok(vec![AiOutput::ToolCall(ToolCall {
                name: "rescope_task".into(),
                arguments: self.args.clone(),
            })])
        }
    }

    #[test]
    fn parse_direction_accepts_canonical_and_synonyms() {
        assert_eq!(ScopeDirection::parse("up").unwrap(), ScopeDirection::Up);
        assert_eq!(
            ScopeDirection::parse("broaden").unwrap(),
            ScopeDirection::Up
        );
        assert_eq!(ScopeDirection::parse("down").unwrap(), ScopeDirection::Down);
        assert_eq!(
            ScopeDirection::parse("narrow").unwrap(),
            ScopeDirection::Down
        );
        assert!(ScopeDirection::parse("sideways").is_err());
    }

    #[test]
    fn up_prompt_contains_task_body_and_broaden_framing() {
        let t = sample_task();
        let p = build_scope_prompt(&t, ScopeDirection::Up);
        assert!(p.contains("Wire login form"));
        assert!(p.contains("/v1/auth/login"));
        assert!(p.contains("Broaden"));
    }

    #[test]
    fn down_prompt_contains_task_body_and_narrow_framing() {
        let t = sample_task();
        let p = build_scope_prompt(&t, ScopeDirection::Down);
        assert!(p.contains("Wire login form"));
        assert!(p.contains("Narrow"));
    }

    #[tokio::test]
    async fn scope_maps_tool_call_to_update_draft() {
        let task = sample_task();
        let fake = FakeProvider {
            args: r#"{"title":"  Ship auth epic  ","description":"broader scope"}"#.into(),
        };
        let draft = scope_task(&fake, &task, ScopeDirection::Up).await.unwrap();
        assert_eq!(draft.id, task.id);
        assert_eq!(draft.patch.title.as_deref(), Some("Ship auth epic"));
        assert_eq!(draft.patch.description.as_deref(), Some("broader scope"));
    }

    #[tokio::test]
    async fn scope_empty_title_is_error() {
        let task = sample_task();
        let fake = FakeProvider {
            args: r#"{"title":"   ","description":"x"}"#.into(),
        };
        let err = scope_task(&fake, &task, ScopeDirection::Down)
            .await
            .unwrap_err();
        assert!(matches!(err, PlanningError::Ai(_)));
    }
}
