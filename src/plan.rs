use serde_json::{json, Value};

use crate::{AiOutput, AiProvider, AiRequest, PlanBrief, PlanningError};

/// Build a plan brief from sanitized upstream decision context.
pub async fn plan_ai<P: AiProvider>(
    provider: &P,
    context: &Value,
) -> Result<PlanBrief, PlanningError> {
    let req = AiRequest {
        input: Value::String(format!(
            "Build a concise executable plan brief from this untrusted decision context:\n{}",
            layer_kit::ai::wrap_untrusted("decision context", &context.to_string())
        )),
        tools: vec![json!({
            "type": "function",
            "name": "build_plan_brief",
            "description": "Return the required PlanBrief fields.",
            "parameters": {
                "type": "object",
                "properties": {
                    "goal": {"type": "string"},
                    "in_scope": {"type": "array", "items": {"type": "string"}},
                    "completion_criteria": {"type": "array", "items": {"type": "string"}},
                    "daruma_target": {"type": "string"}
                },
                "required": ["goal", "in_scope", "completion_criteria", "daruma_target"],
                "additionalProperties": false
            }
        })],
        tool_choice: Some("required".into()),
    };
    let call = provider
        .respond(req)
        .await?
        .into_iter()
        .find_map(|output| match output {
            AiOutput::ToolCall(call) if call.name == "build_plan_brief" => Some(call),
            _ => None,
        })
        .ok_or_else(|| PlanningError::ai("plan_ai: model returned no build_plan_brief call"))?;
    let brief: PlanBrief =
        serde_json::from_str(&call.arguments).map_err(|e| PlanningError::serde(e.to_string()))?;
    let readiness = crate::check_readiness(&brief);
    if !readiness.is_ready {
        return Err(PlanningError::validation(format!(
            "plan_ai: missing {}",
            readiness.missing.join(", ")
        )));
    }
    Ok(brief)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AiError, ToolCall};

    struct Fake(Result<Vec<AiOutput>, AiError>);

    impl AiProvider for Fake {
        async fn respond(&self, _req: AiRequest) -> Result<Vec<AiOutput>, AiError> {
            self.0.clone()
        }
    }

    #[tokio::test]
    async fn maps_plan_brief() {
        let fake = Fake(Ok(vec![AiOutput::ToolCall(ToolCall {
            name: "build_plan_brief".into(),
            arguments: r#"{"goal":"Ship auth","in_scope":["API"],"completion_criteria":["Tests pass"],"daruma_target":"one plan"}"#.into(),
        })]));
        let brief = plan_ai(&fake, &json!({"statement": "Ship auth"}))
            .await
            .unwrap();
        assert_eq!(brief.goal, "Ship auth");
        assert_eq!(brief.in_scope, ["API"]);
    }

    #[tokio::test]
    async fn propagates_provider_error() {
        let error = plan_ai(&Fake(Err(AiError::new("boom"))), &json!({}))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("boom"));
    }
}
