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
use layer_kit::ai::extract_ai_config;
use layer_kit::auth::Claims;
use layer_kit::openai::{AiConfig, OpenAiProvider};
use layer_kit::serve::{serve, McpHandler, ServeConfig};
use layer_kit::store::Store;
use serde_json::json;
use yatagarasu::{PlanBrief, ScopeDirection, Task, TaskBrief, TaskId};

const TOOL: &str = "yatagarasu";

/// Dispatches yatagarasu's MCP methods; owns the (optional) AI provider.
struct Handler {
    /// `None` when OPENAI_API_KEY is unset — AI methods then answer
    /// `ai_not_configured` instead of panicking at call time.
    ai: Option<OpenAiProvider>,
    store: Store,
}

impl McpHandler for Handler {
    async fn dispatch(
        &self,
        _claims: &Claims,
        method: &str,
        mut params: serde_json::Value,
    ) -> Result<serde_json::Value, (StatusCode, serde_json::Value)> {
        if let Some(cfg) = extract_ai_config(&mut params) {
            let provider = OpenAiProvider::new(cfg);
            dispatch_with_ai(
                &self.store,
                Some(&provider),
                Some(provider.model()),
                method,
                params,
            )
            .await
        } else {
            dispatch(&self.store, self.ai.as_ref(), method, params).await
        }
    }

    fn tools(&self) -> Vec<serde_json::Value> {
        tools()
    }
}

/// Tool descriptors for `tools/list` — one per method actually handled by
/// [`dispatch`] (`yatagarasu.enrich` remains unsupported).
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
        json!({
            "name": "yatagarasu_read",
            "description": "Get a persisted PlanBrief by its source decision id.",
            "inputSchema": {
                "type": "object",
                "properties": {"id": {"type": "string"}},
                "required": ["id"]
            }
        }),
        json!({
            "name": "yatagarasu_enrich",
            "description": "Enrich a plan through a host-provided adapter.",
            "inputSchema": {"type": "object", "properties": {}}
        }),
    ]
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().json().init();

    let ai = AiConfig::from_env().map(OpenAiProvider::new);
    if ai.is_none() {
        tracing::warn!(
            "OPENAI_API_KEY unset — env-backed AI methods will answer ai_not_configured"
        );
    }
    let store = Store::from_env(TOOL).await.unwrap_or_else(|e| {
        tracing::error!(error = %e, "failed to open yatagarasu store");
        std::process::exit(1);
    });

    serve(
        ServeConfig {
            tool: TOOL,
            default_port: 8093,
            default_version: env!("CARGO_PKG_VERSION"),
            git_sha: option_env!("GIT_SHA").unwrap_or("dev"),
        },
        Handler { ai, store },
    )
    .await;
}

/// Params for `yatagarasu.plan`. `source_ref` is the upstream Decision id,
/// recorded in `PlanBrief.decisions_made` so lineage survives the network hop.
#[derive(serde::Deserialize)]
struct PlanParams {
    source_ref: String,
}

#[derive(serde::Deserialize)]
struct ReadParams {
    id: String,
}

fn storage_error(e: impl std::fmt::Display) -> (StatusCode, serde_json::Value) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        json!({"error": "storage_error", "detail": e.to_string()}),
    )
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

const METHODS: &[&str] = &[
    "yatagarasu.plan",
    "yatagarasu.decompose",
    "yatagarasu.scope",
    "yatagarasu.analyze_complexity",
    "yatagarasu.read",
    "yatagarasu.enrich",
];

/// Pure MCP dispatch over the yatagarasu planning lib — no auth, no HTTP, so
/// it is unit-testable directly (AI methods get a fake `AiProvider` in
/// tests). Read/enrichment methods belong to host adapters, not this
/// stateless server.
async fn dispatch<P: yatagarasu::AiProvider>(
    store: &Store,
    ai: Option<&P>,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, (StatusCode, serde_json::Value)> {
    dispatch_with_ai(store, ai, None, method, params).await
}

async fn dispatch_with_ai<P: yatagarasu::AiProvider>(
    store: &Store,
    ai: Option<&P>,
    model: Option<&str>,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, (StatusCode, serde_json::Value)> {
    if !METHODS.contains(&method) {
        return Err((
            StatusCode::BAD_REQUEST,
            json!({"error": "unknown_method", "detail": method}),
        ));
    }
    match method {
        "yatagarasu.plan" => {
            let context = params.clone();
            let p: PlanParams = serde_json::from_value(params).map_err(invalid_params)?;
            let store_id = p.source_ref.clone();
            let (brief, usage) = if let Some(model) = model {
                let provider = ai.ok_or_else(ai_not_configured)?;
                let (mut brief, usage) = yatagarasu::plan_ai(provider, &context).await.map_err(|e| {
                    (
                        StatusCode::BAD_GATEWAY,
                        json!({"error": "ai_error", "detail": e.to_string()}),
                    )
                })?;
                brief.decisions_made = vec![p.source_ref];
                (brief, Some((model, usage)))
            } else {
                (yatagarasu::brief_from_decisions(&[p.source_ref]), None)
            };
            store
                .put("plan_brief", &store_id, &brief)
                .await
                .map_err(storage_error)?;
            let mut out = json!({ "method": "yatagarasu.plan", "plan_brief": brief });
            for field in ["decision", "sensing_item"] {
                if let Some(value) = context.get(field) {
                    out[field] = value.clone();
                }
            }
            if let Some((model, usage)) = usage {
                let mut meta = json!({"model": model});
                if let Some(usage) = usage {
                    meta["usage"] = json!(usage);
                }
                out["_meta"] = meta;
            }
            Ok(out)
        }
        "yatagarasu.decompose" => {
            let p: DecomposeParams = serde_json::from_value(params).map_err(invalid_params)?;
            let parent: TaskId = p.parent.parse().map_err(invalid_params)?;
            let Some(provider) = ai else {
                return Err(ai_not_configured());
            };
            // Real AI operation: task context → SplitDraft (≥2 sub-tasks).
            let draft =
                yatagarasu::decompose_task(provider, parent, &p.task_context, p.hint.as_deref())
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
        "yatagarasu.read" => {
            let p: ReadParams = serde_json::from_value(params).map_err(invalid_params)?;
            let brief: Option<PlanBrief> = store
                .get("plan_brief", &p.id)
                .await
                .map_err(storage_error)?;
            brief
                .map(|brief| json!({"method": "yatagarasu.read", "plan_brief": brief}))
                .ok_or_else(|| {
                    (
                        StatusCode::NOT_FOUND,
                        json!({"error": "not_found", "detail": p.id}),
                    )
                })
        }
        "yatagarasu.enrich" => Err((
            StatusCode::NOT_IMPLEMENTED,
            json!({"error": "unsupported", "detail": "yatagarasu.enrich needs a host adapter"}),
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
    use std::sync::atomic::{AtomicU64, Ordering};
    use yatagarasu::{AiError, AiOutput, AiRequest, AiUsage, ToolCall};

    static DB_SEQ: AtomicU64 = AtomicU64::new(1);

    fn db_path() -> String {
        std::env::temp_dir()
            .join(format!(
                "yatagarasu-server-{}-{}.db",
                std::process::id(),
                DB_SEQ.fetch_add(1, Ordering::Relaxed)
            ))
            .to_string_lossy()
            .into_owned()
    }

    async fn test_store() -> Store {
        Store::open(&db_path()).await.unwrap()
    }

    async fn dispatch<P: yatagarasu::AiProvider>(
        ai: Option<&P>,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, (StatusCode, serde_json::Value)> {
        super::dispatch(&test_store().await, ai, method, params).await
    }

    async fn dispatch_with_ai<P: yatagarasu::AiProvider>(
        ai: Option<&P>,
        request_ai: bool,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, (StatusCode, serde_json::Value)> {
        super::dispatch_with_ai(
            &test_store().await,
            ai,
            request_ai.then_some("test"),
            method,
            params,
        )
        .await
    }

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

        async fn respond_with_usage(
            &self,
            req: AiRequest,
        ) -> Result<(Vec<AiOutput>, Option<AiUsage>), AiError> {
            Ok((self.respond(req).await?, Some(AiUsage {
                input_tokens: Some(123),
                output_tokens: Some(45),
                total_tokens: Some(168),
            })))
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
        assert!(out.get("_meta").is_none());
    }

    #[tokio::test]
    async fn request_ai_builds_plan_without_leaking_secret() {
        let fake = FakeTool {
            name: "build_plan_brief",
            args: r#"{"goal":"Ship auth","in_scope":["API"],"completion_criteria":["Tests pass"],"daruma_target":"one plan"}"#.into(),
        };
        let mut params = json!({
            "source_ref": "decision_abc",
            "decision": {"statement": "Ship auth"},
            "ai": {"api_key": "sk-secret", "base_url": "https://ai.test/v1", "model": "test"}
        });
        assert!(extract_ai_config(&mut params).is_some());
        let store = test_store().await;
        let out = super::dispatch_with_ai(
            &store,
            Some(&fake),
            Some("test"),
            "yatagarasu.plan",
            params,
        )
            .await
            .unwrap();
        assert_eq!(out["plan_brief"]["goal"], "Ship auth");
        assert_eq!(out["plan_brief"]["decisions_made"], json!(["decision_abc"]));
        assert_eq!(out["decision"]["statement"], "Ship auth");
        assert_eq!(out["_meta"]["model"], "test");
        assert_eq!(out["_meta"]["usage"]["total_tokens"], 168);
        let stored: serde_json::Value = store
            .get("plan_brief", "decision_abc")
            .await
            .unwrap()
            .unwrap();
        assert!(stored.get("_meta").is_none());
        assert!(!out.to_string().contains("sk-secret"));
    }

    #[tokio::test]
    async fn request_ai_plan_failure_is_ai_error() {
        struct Failing;
        impl yatagarasu::AiProvider for Failing {
            async fn respond(&self, _req: AiRequest) -> Result<Vec<AiOutput>, AiError> {
                Err(AiError::new("boom"))
            }
        }
        let (code, body) = dispatch_with_ai(
            Some(&Failing),
            true,
            "yatagarasu.plan",
            json!({"source_ref": "decision_abc"}),
        )
        .await
        .unwrap_err();
        assert_eq!(code, StatusCode::BAD_GATEWAY);
        assert_eq!(body["error"], "ai_error");
    }

    #[tokio::test]
    async fn read_and_unknown_method_rejected() {
        let (code, _) = dispatch(None::<&OpenAiProvider>, "yatagarasu.read", json!({}))
            .await
            .unwrap_err();
        assert_eq!(code, StatusCode::BAD_REQUEST);
        let (code, _) = dispatch(None::<&OpenAiProvider>, "yatagarasu.nope", json!({}))
            .await
            .unwrap_err();
        assert_eq!(code, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn plan_brief_persists_across_restart_and_write_errors_surface() {
        let path = db_path();
        let store = Store::open(&path).await.unwrap();
        super::dispatch(
            &store,
            None::<&OpenAiProvider>,
            "yatagarasu.plan",
            json!({"source_ref": "decision_1"}),
        )
        .await
        .unwrap();
        drop(store);

        let reopened = Store::open(&path).await.unwrap();
        let got = super::dispatch(
            &reopened,
            None::<&OpenAiProvider>,
            "yatagarasu.read",
            json!({"id": "decision_1"}),
        )
        .await
        .unwrap();
        assert_eq!(got["plan_brief"]["decisions_made"], json!(["decision_1"]));

        reopened.pool().close().await;
        let (code, body) = super::dispatch(
            &reopened,
            None::<&OpenAiProvider>,
            "yatagarasu.plan",
            json!({"source_ref": "decision_2"}),
        )
        .await
        .unwrap_err();
        assert_eq!(code, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"], "storage_error");
    }

    #[tokio::test]
    async fn tools_list_names_are_all_dispatchable() {
        for tool in tools() {
            let name = tool["name"].as_str().unwrap();
            let method = name.replacen('_', ".", 1);
            if let Err((_, body)) = dispatch(None::<&OpenAiProvider>, &method, json!({})).await {
                assert_ne!(body["error"], "unknown_method", "{method} must be real");
            }
        }
    }

    #[test]
    fn tools_catalogue_matches_methods() {
        layer_kit::test_support::assert_catalogue_matches(&tools(), METHODS);
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
