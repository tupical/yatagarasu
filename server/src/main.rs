//! yatagarasu-server — thin, independently-deployed HTTP/MCP wrapper around the
//! `yatagarasu` planning lib. Its own deploy unit (own systemd service, own
//! port). Boundary-clean: no mcpbox dependency; the platform→tool auth
//! contract and the axum/tokio scaffold live in `layer_kit::{auth,serve}`.
//!
//! Routes:
//!   GET  /healthz   — open; liveness + version for the platform registry.
//!   POST /v1/mcp    — requires a valid platform token; planning surface
//!                     (`yatagarasu.plan` builds a typed PlanBrief via the lib;
//!                     `yatagarasu.decompose` / `yatagarasu.scope` /
//!                     `yatagarasu.analyze_complexity` run the lib's AI
//!                     planning operations).
//!
//! Env: YATAGARASU_PORT (default 8093), YATAGARASU_PLATFORM_SECRET (HMAC key;
//! if unset, /v1/mcp is closed), YATAGARASU_VERSION (defaults to the crate
//! version). AI methods: OPENAI_API_KEY / OPENAI_BASE_URL / OPENAI_MODEL
//! (see `layer_kit::openai`); without a key they answer `ai_not_configured`.

use axum::http::StatusCode;
use layer_kit::auth::Claims;
use layer_kit::openai::{AiConfig, OpenAiProvider};
use layer_kit::serve::{serve, McpHandler, ServeConfig};
use serde_json::json;
use yatagarasu::{PlanBrief, ScopeDirection, Task, TaskBrief, TaskId};

const TOOL: &str = "yatagarasu";

/// Dispatches yatagarasu's MCP methods; owns the (optional) AI provider.
struct Handler {
    /// `None` when OPENAI_API_KEY is unset — AI methods then answer
    /// `ai_not_configured` instead of panicking at call time.
    ai: Option<OpenAiProvider>,
}

impl McpHandler for Handler {
    async fn dispatch(
        &self,
        _claims: &Claims,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, (StatusCode, serde_json::Value)> {
        dispatch(self.ai.as_ref(), method, params).await
    }

    fn tools(&self) -> Vec<serde_json::Value> {
        tools()
    }
}

/// Tool descriptors for `tools/list` — one per method actually handled by
/// [`dispatch`] (`yatagarasu.read`/`yatagarasu.enrich` are NOT_IMPLEMENTED,
/// so they are omitted).
fn tools() -> Vec<serde_json::Value> {
    vec![
        json!({
            "name": "yatagarasu_plan",
            "description": "Build a typed PlanBrief linked to an upstream Decision.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "source_ref": {"type": "string"}
                },
                "required": ["source_ref"]
            }
        }),
        json!({
            "name": "yatagarasu_decompose",
            "description": "AI decomposition: split a parent task into an ordered set of sub-task drafts.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "parent": {"type": "string"},
                    "task_context": {"type": "string"},
                    "hint": {"type": "string"}
                },
                "required": ["parent", "task_context"]
            }
        }),
        json!({
            "name": "yatagarasu_scope",
            "description": "AI rescope: rewrite a task's title/description broader or narrower.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task": {"type": "object"},
                    "direction": {"type": "string"}
                },
                "required": ["task", "direction"]
            }
        }),
        json!({
            "name": "yatagarasu_analyze_complexity",
            "description": "Batch AI scoring: one model call producing a ComplexityHintDraft per task.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "tasks": {"type": "array", "items": {"type": "object"}}
                },
                "required": ["tasks"]
            }
        }),
    ]
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().json().init();

    let ai = AiConfig::from_env().map(OpenAiProvider::new);
    if ai.is_none() {
        tracing::warn!("OPENAI_API_KEY unset — AI methods (yatagarasu.decompose/scope/analyze_complexity) will answer ai_not_configured");
    }

    serve(
        ServeConfig {
            tool: TOOL,
            default_port: 8093,
            default_version: env!("CARGO_PKG_VERSION"),
            git_sha: option_env!("GIT_SHA").unwrap_or("dev"),
        },
        Handler { ai },
    )
    .await;
}

/// Params for `yatagarasu.plan`. `source_ref` is the upstream Decision id,
/// recorded in `PlanBrief.decisions_made` so lineage survives the network hop.
#[derive(serde::Deserialize)]
struct PlanParams {
    source_ref: String,
}

/// Params for `yatagarasu.decompose` — the lib's AI decomposition
/// ([`decompose_task`](yatagarasu::decompose_task)): a parent task → an
/// ordered set of ≥2 sub-task drafts (`SplitDraft`).
#[derive(serde::Deserialize)]
struct DecomposeParams {
    /// Parent task id (`task_<uuid>` or bare uuid).
    parent: String,
    /// Title + description (whatever gives the model enough to split well).
    task_context: String,
    /// Optional free-form guidance appended to the prompt.
    #[serde(default)]
    hint: Option<String>,
}

/// Params for `yatagarasu.scope` — the lib's AI rescope
/// ([`scope_task`](yatagarasu::scope_task)): rewrite a task's title +
/// description broader (`up`) or narrower (`down`), as an `UpdateDraft`.
#[derive(serde::Deserialize)]
struct ScopeParams {
    task: ScopeTaskInput,
    /// `up`/`broaden` or `down`/`narrow`.
    direction: String,
}

/// Minimal task shape `scope` needs; the server rebuilds the lib's `Task`
/// (status/priority/timestamps are unused by the operation).
#[derive(serde::Deserialize)]
struct ScopeTaskInput {
    id: String,
    title: String,
    #[serde(default)]
    description: String,
}

/// Params for `yatagarasu.analyze_complexity` — the lib's batch AI scoring
/// ([`analyze_complexity_batch`](yatagarasu::analyze_complexity_batch)):
/// one model call → one `ComplexityHintDraft` per task.
#[derive(serde::Deserialize)]
struct AnalyzeParams {
    tasks: Vec<AnalyzeTaskInput>,
}

#[derive(serde::Deserialize)]
struct AnalyzeTaskInput {
    task_id: String,
    title: String,
    #[serde(default)]
    description: String,
}

fn invalid_params(e: impl std::fmt::Display) -> (StatusCode, serde_json::Value) {
    (
        StatusCode::BAD_REQUEST,
        json!({"error": "invalid_params", "detail": e.to_string()}),
    )
}

/// Error when no AI provider is configured: an honest 503, not a panic.
fn ai_not_configured() -> (StatusCode, serde_json::Value) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        json!({"error": "ai_not_configured", "detail": "OPENAI_API_KEY not set; yatagarasu-server has no AI provider"}),
    )
}

/// Map a lib [`PlanningError`](yatagarasu::PlanningError) onto the wire:
/// caller input problems → 400, provider/upstream problems → 502.
fn ai_error(e: yatagarasu::PlanningError) -> (StatusCode, serde_json::Value) {
    match e {
        yatagarasu::PlanningError::Validation(m) => (
            StatusCode::BAD_REQUEST,
            json!({"error": "validation", "detail": m}),
        ),
        other => (
            StatusCode::BAD_GATEWAY,
            json!({"error": "ai_upstream", "detail": other.to_string()}),
        ),
    }
}

/// Pure MCP dispatch over the yatagarasu planning lib — no auth, no HTTP, so
/// it is unit-testable directly (AI methods get a fake `AiProvider` in
/// tests). Read/enrichment methods belong to host adapters, not this
/// stateless server.
async fn dispatch<P: yatagarasu::AiProvider>(
    ai: Option<&P>,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, (StatusCode, serde_json::Value)> {
    match method {
        "yatagarasu.plan" => {
            let p: PlanParams = serde_json::from_value(params).map_err(invalid_params)?;
            let brief = PlanBrief {
                decisions_made: vec![p.source_ref],
                ..PlanBrief::default()
            };
            Ok(json!({ "method": "yatagarasu.plan", "plan_brief": brief }))
        }
        "yatagarasu.decompose" => {
            let p: DecomposeParams = serde_json::from_value(params).map_err(invalid_params)?;
            let parent: TaskId = p.parent.parse().map_err(invalid_params)?;
            let Some(provider) = ai else {
                return Err(ai_not_configured());
            };
            // Real AI operation: task context → SplitDraft (≥2 sub-tasks).
            let draft = yatagarasu::decompose_task(
                provider,
                parent,
                &p.task_context,
                p.hint.as_deref(),
            )
            .await
            .map_err(ai_error)?;
            Ok(json!({ "method": "yatagarasu.decompose", "split_draft": draft }))
        }
        "yatagarasu.scope" => {
            let p: ScopeParams = serde_json::from_value(params).map_err(invalid_params)?;
            let direction = ScopeDirection::parse(&p.direction).map_err(ai_error)?;
            let id: TaskId = p.task.id.parse().map_err(invalid_params)?;
            let Some(provider) = ai else {
                return Err(ai_not_configured());
            };
            // Rebuild the lib's Task; only id/title/description feed the op.
            let now = yatagarasu::time::now();
            let task = Task {
                id,
                project_id: None,
                title: p.task.title,
                description: p.task.description,
                status: Default::default(),
                priority: Default::default(),
                created_at: now,
                updated_at: now,
            };
            // Real AI operation: task → UpdateDraft (rewritten title/desc).
            let draft = yatagarasu::scope_task(provider, &task, direction)
                .await
                .map_err(ai_error)?;
            Ok(json!({ "method": "yatagarasu.scope", "update_draft": draft }))
        }
        "yatagarasu.analyze_complexity" => {
            let p: AnalyzeParams = serde_json::from_value(params).map_err(invalid_params)?;
            let mut tasks = Vec::with_capacity(p.tasks.len());
            for t in p.tasks {
                tasks.push(TaskBrief {
                    task_id: t.task_id.parse().map_err(invalid_params)?,
                    title: t.title,
                    description: t.description,
                });
            }
            let Some(provider) = ai else {
                return Err(ai_not_configured());
            };
            // Real AI operation: one batch call → ComplexityHintDraft per task.
            let hints = yatagarasu::analyze_complexity_batch(provider, tasks)
                .await
                .map_err(ai_error)?;
            Ok(json!({ "method": "yatagarasu.analyze_complexity", "hints": hints }))
        }
        "yatagarasu.read" | "yatagarasu.enrich" => Err((
            StatusCode::NOT_IMPLEMENTED,
            json!({"error": "unsupported", "detail": "yatagarasu-server is stateless (OSS skeleton has no store); read/enrich need a host adapter"}),
        )),
        other => Err((
            StatusCode::BAD_REQUEST,
            json!({"error": "unknown_method", "detail": other}),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yatagarasu::{AiError, AiOutput, AiRequest, ToolCall};

    /// Fake provider returning a fixed tool call — lets dispatch tests
    /// exercise the AI methods without network.
    struct FakeTool {
        name: &'static str,
        args: String,
    }

    impl yatagarasu::AiProvider for FakeTool {
        async fn respond(&self, _req: AiRequest) -> Result<Vec<AiOutput>, AiError> {
            Ok(vec![AiOutput::ToolCall(ToolCall {
                name: self.name.into(),
                arguments: self.args.clone(),
            })])
        }
    }

    #[tokio::test]
    async fn plan_builds_plan_brief_with_decision_provenance() {
        let out = dispatch(
            None::<&OpenAiProvider>,
            "yatagarasu.plan",
            json!({"source_ref": "decision_abc"}),
        )
        .await
        .expect("plan must succeed");
        let brief = &out["plan_brief"];
        assert_eq!(out["method"], "yatagarasu.plan");
        assert_eq!(brief["decisions_made"], json!(["decision_abc"]));
    }

    #[tokio::test]
    async fn read_unsupported_and_unknown_method_rejected() {
        let (code, _) = dispatch(None::<&OpenAiProvider>, "yatagarasu.read", json!({}))
            .await
            .unwrap_err();
        assert_eq!(code, StatusCode::NOT_IMPLEMENTED);
        let (code, _) = dispatch(None::<&OpenAiProvider>, "yatagarasu.nope", json!({}))
            .await
            .unwrap_err();
        assert_eq!(code, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn tools_list_names_are_all_dispatchable() {
        for tool in tools() {
            let name = tool["name"].as_str().unwrap();
            let method = name.replacen('_', ".", 1);
            let (_, body) = dispatch(None::<&OpenAiProvider>, &method, json!({}))
                .await
                .expect_err("empty params must not satisfy any real method");
            assert_ne!(
                body["error"], "unknown_method",
                "{method} must be a real dispatch method"
            );
        }
    }

    #[tokio::test]
    async fn plan_rejects_bad_params() {
        let (code, _) = dispatch(None::<&OpenAiProvider>, "yatagarasu.plan", json!({}))
            .await
            .unwrap_err();
        assert_eq!(code, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn decompose_maps_tool_call_to_split_draft() {
        let parent = TaskId::new();
        let fake = FakeTool {
            name: "split_task",
            args: r#"{"subtasks":[{"title":"Design schema","description":"ERD"},{"title":"Wire API"}]}"#
                .into(),
        };
        let out = dispatch(
            Some(&fake),
            "yatagarasu.decompose",
            json!({"parent": parent.to_string(), "task_context": "Build login page"}),
        )
        .await
        .expect("decompose must succeed");
        assert_eq!(out["method"], "yatagarasu.decompose");
        let draft = &out["split_draft"];
        assert_eq!(draft["parent"], json!(parent.0));
        let subs = draft["subtasks"].as_array().unwrap();
        assert_eq!(subs.len(), 2);
        assert_eq!(subs[0]["title"], "Design schema");
        assert_eq!(subs[0]["description"], "ERD");
        assert_eq!(subs[1]["title"], "Wire API");
    }

    #[tokio::test]
    async fn decompose_rejects_bad_params_and_bad_parent_id() {
        let fake = FakeTool {
            name: "split_task",
            args: "{}".into(),
        };
        let (code, body) = dispatch(Some(&fake), "yatagarasu.decompose", json!({}))
            .await
            .unwrap_err();
        assert_eq!(code, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_params");

        let (code, _) = dispatch(
            Some(&fake),
            "yatagarasu.decompose",
            json!({"parent": "not-a-uuid", "task_context": "x"}),
        )
        .await
        .unwrap_err();
        assert_eq!(code, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn scope_maps_tool_call_to_update_draft() {
        let id = TaskId::new();
        let fake = FakeTool {
            name: "rescope_task",
            args: r#"{"title":"Ship auth epic","description":"broader scope"}"#.into(),
        };
        let out = dispatch(
            Some(&fake),
            "yatagarasu.scope",
            json!({
                "task": {"id": id.to_string(), "title": "Wire login form", "description": "Connect to /v1/auth/login"},
                "direction": "up"
            }),
        )
        .await
        .expect("scope must succeed");
        let draft = &out["update_draft"];
        assert_eq!(draft["id"], json!(id.0));
        assert_eq!(draft["patch"]["title"], "Ship auth epic");
        assert_eq!(draft["patch"]["description"], "broader scope");
    }

    #[tokio::test]
    async fn scope_rejects_unknown_direction() {
        let fake = FakeTool {
            name: "rescope_task",
            args: "{}".into(),
        };
        let (code, body) = dispatch(
            Some(&fake),
            "yatagarasu.scope",
            json!({"task": {"id": TaskId::new().to_string(), "title": "t"}, "direction": "sideways"}),
        )
        .await
        .unwrap_err();
        assert_eq!(code, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "validation");
    }

    #[tokio::test]
    async fn analyze_complexity_maps_hints() {
        let a = TaskId::new();
        let b = TaskId::new();
        let args = format!(
            r#"{{"hints":[
                {{"task_id":"{a}","score":9,"recommended_subtasks":5,"expansion_hint":"split DB","reasoning":"big"}},
                {{"task_id":"{b}","score":2,"recommended_subtasks":0,"expansion_hint":"ship as-is","reasoning":"small"}}
            ]}}"#
        );
        let fake = FakeTool {
            name: "report_complexity",
            args,
        };
        let out = dispatch(
            Some(&fake),
            "yatagarasu.analyze_complexity",
            json!({"tasks": [
                {"task_id": a.to_string(), "title": "Wire DB layer"},
                {"task_id": b.to_string(), "title": "Add MCP tool", "description": "two lines"}
            ]}),
        )
        .await
        .expect("analyze_complexity must succeed");
        assert_eq!(out["method"], "yatagarasu.analyze_complexity");
        let hints = out["hints"].as_array().unwrap();
        assert_eq!(hints.len(), 2);
        assert_eq!(hints[0]["task_id"], json!(a.0));
        assert_eq!(hints[0]["score"], 9);
        assert_eq!(hints[0]["recommended_subtasks"], 5);
        assert_eq!(hints[1]["task_id"], json!(b.0));
    }

    #[tokio::test]
    async fn analyze_complexity_rejects_bad_task_id() {
        let fake = FakeTool {
            name: "report_complexity",
            args: "{}".into(),
        };
        let (code, body) = dispatch(
            Some(&fake),
            "yatagarasu.analyze_complexity",
            json!({"tasks": [{"task_id": "nope", "title": "t"}]}),
        )
        .await
        .unwrap_err();
        assert_eq!(code, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_params");
    }

    #[tokio::test]
    async fn ai_methods_without_provider_are_honest_503() {
        for (method, params) in [
            (
                "yatagarasu.decompose",
                json!({"parent": TaskId::new().to_string(), "task_context": "x"}),
            ),
            (
                "yatagarasu.scope",
                json!({"task": {"id": TaskId::new().to_string(), "title": "t"}, "direction": "up"}),
            ),
            (
                "yatagarasu.analyze_complexity",
                json!({"tasks": [{"task_id": TaskId::new().to_string(), "title": "t"}]}),
            ),
        ] {
            let (code, body) = dispatch(None::<&OpenAiProvider>, method, params)
                .await
                .unwrap_err();
            assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE, "{method}");
            assert_eq!(body["error"], "ai_not_configured", "{method}");
        }
    }

    #[tokio::test]
    async fn ai_provider_failure_is_502() {
        struct Failing;
        impl yatagarasu::AiProvider for Failing {
            async fn respond(&self, _req: AiRequest) -> Result<Vec<AiOutput>, AiError> {
                Err(AiError::new("boom"))
            }
        }
        let (code, body) = dispatch(
            Some(&Failing),
            "yatagarasu.decompose",
            json!({"parent": TaskId::new().to_string(), "task_context": "x"}),
        )
        .await
        .unwrap_err();
        assert_eq!(code, StatusCode::BAD_GATEWAY);
        assert_eq!(body["error"], "ai_upstream");
    }
}
