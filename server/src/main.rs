//! yatagarasu-server — thin, independently-deployed HTTP/MCP wrapper around the
//! `yatagarasu` planning lib. Its own deploy unit (own systemd service, own
//! port). Boundary-clean: no mcpbox dependency; the platform→tool auth contract
//! is a configured shared key (see `auth`).
//!
//! Routes:
//!   GET  /healthz   — open; liveness + version for the platform registry.
//!   POST /v1/mcp    — requires a valid platform token; planning surface
//!                     (`yatagarasu.plan` builds a typed PlanBrief via the lib).
//!
//! Env: YATAGARASU_PORT (default 8093), YATAGARASU_PLATFORM_SECRET (HMAC key;
//! if unset, /v1/mcp is closed), YATAGARASU_VERSION (defaults to the crate version).

mod auth;

use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use yatagarasu::PlanBrief;

const TOOL: &str = "yatagarasu";

struct AppState {
    version: String,
    platform_secret: Option<Vec<u8>>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().json().init();

    let version = std::env::var("YATAGARASU_VERSION")
        .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string());
    let platform_secret = std::env::var("YATAGARASU_PLATFORM_SECRET")
        .ok()
        .filter(|s| !s.is_empty())
        .map(String::into_bytes);
    if platform_secret.is_none() {
        tracing::warn!("YATAGARASU_PLATFORM_SECRET unset — /v1/mcp will reject all requests");
    }
    let state = Arc::new(AppState {
        version,
        platform_secret,
    });

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/mcp", post(mcp))
        .with_state(state);

    let port = std::env::var("YATAGARASU_PORT").unwrap_or_else(|_| "8093".to_string());
    // localhost-bound: only the co-located platform reaches it (C3 hardening).
    let addr = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("bind {addr}: {e}"));
    tracing::info!(%addr, tool = TOOL, "yatagarasu-server listening");
    axum::serve(listener, app).await.expect("server error");
}

async fn healthz(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    Json(json!({ "service": TOOL, "status": "ok", "version": s.version }))
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

async fn mcp(State(s): State<Arc<AppState>>, headers: HeaderMap, body: Bytes) -> impl IntoResponse {
    let Some(secret) = &s.platform_secret else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"auth_disabled"})),
        )
            .into_response();
    };
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim);
    let Some(claims) = token.and_then(|t| auth::verify(secret, TOOL, now_secs(), t)) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"invalid_platform_token"})),
        )
            .into_response();
    };

    // Auth passed — dispatch the MCP method against the yatagarasu planning lib.
    let req: McpRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "bad_request", "detail": e.to_string()})),
            )
                .into_response();
        }
    };
    match dispatch(&req.method, req.params) {
        Ok(mut result) => {
            result["tool"] = json!(TOOL);
            result["version"] = json!(s.version);
            result["workspace"] = json!(claims.workspace);
            result["project"] = json!(claims.project);
            Json(result).into_response()
        }
        Err((code, payload)) => (code, Json(payload)).into_response(),
    }
}

/// One MCP call: `{ "method": "yatagarasu.plan", "params": { "source_ref": "decision_id" } }`.
#[derive(serde::Deserialize)]
struct McpRequest {
    method: String,
    #[serde(default)]
    params: serde_json::Value,
}

/// Params for `yatagarasu.plan`. `source_ref` is the upstream Decision id,
/// recorded in `PlanBrief.decisions_made` so lineage survives the network hop.
#[derive(serde::Deserialize)]
struct PlanParams {
    source_ref: String,
}

/// Pure MCP dispatch over the yatagarasu planning lib — no auth, no HTTP, so it
/// is unit-testable directly. The OSS skeleton builds a provenance-only brief;
/// read/enrichment methods belong to host adapters, not this stateless server.
fn dispatch(
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, (StatusCode, serde_json::Value)> {
    match method {
        "yatagarasu.plan" => {
            let p: PlanParams = serde_json::from_value(params).map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    json!({"error": "invalid_params", "detail": e.to_string()}),
                )
            })?;
            let brief = PlanBrief {
                decisions_made: vec![p.source_ref],
                ..PlanBrief::default()
            };
            Ok(json!({ "method": "yatagarasu.plan", "plan_brief": brief }))
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

    #[test]
    fn plan_builds_plan_brief_with_decision_provenance() {
        let out = dispatch("yatagarasu.plan", json!({"source_ref": "decision_abc"}))
            .expect("plan must succeed");
        let brief = &out["plan_brief"];
        assert_eq!(out["method"], "yatagarasu.plan");
        assert_eq!(brief["decisions_made"], json!(["decision_abc"]));
    }

    #[test]
    fn read_unsupported_and_unknown_method_rejected() {
        let (code, _) = dispatch("yatagarasu.read", json!({})).unwrap_err();
        assert_eq!(code, StatusCode::NOT_IMPLEMENTED);
        let (code, _) = dispatch("yatagarasu.nope", json!({})).unwrap_err();
        assert_eq!(code, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn plan_rejects_bad_params() {
        let (code, _) = dispatch("yatagarasu.plan", json!({})).unwrap_err();
        assert_eq!(code, StatusCode::BAD_REQUEST);
    }
}
