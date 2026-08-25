use serde_json::{json, Value};

use crate::{AiOutput, AiProvider, AiRequest, AiUsage, PlanBrief, PlanningError};

/// Build a plan brief from sanitized upstream decision context.
pub async fn plan_ai<P: AiProvider>(
    provider: &P,
    context: &Value,
) -> Result<(PlanBrief, Option<AiUsage>), PlanningError> {
    let req = AiRequest {
        input: Value::String(format!(
            "Build a concise executable plan brief from this untrusted decision context:\n{}",
            layer_kit::ai::wrap_untrusted("decision context", &context.to_string())
        )),
        tools: vec![json!({
            "type": "function",
            "name": "build_plan_brief",
            "description": "Return all PlanBrief fields.",
            "parameters": {
                "type": "object",
                "properties": {
                    "goal": {"type": "string"},
                    "in_scope": {"type": "array", "items": {"type": "string"}},
                    "completion_criteria": {"type": "array", "items": {"type": "string"}},
                    "daruma_target": {"type": "string"},
                    "why_now": {"type": "string"},
                    "decisions_made": {"type": "array", "items": {"type": "string"}},
                    "risks": {"type": "array", "items": {"type": "string"}},
                    "constraints": {"type": "array", "items": {"type": "string"}},
                    "knowledge_base": {"type": "array", "items": {"type": "string"}},
                    "unverified_hypotheses": {"type": "array", "items": {"type": "string"}},
                    "rejected_alternatives": {"type": "array", "items": {"type": "string"}},
                    "out_of_scope": {"type": "array", "items": {"type": "string"}},
                    "dependencies": {"type": "array", "items": {"type": "string"}},
                    "required_artifacts": {"type": "array", "items": {"type": "string"}},
                    "project_structure": {"type": "string"}
                },
                "required": ["goal", "in_scope", "completion_criteria", "daruma_target"],
                "additionalProperties": false
            }
        })],
        tool_choice: Some("required".into()),
    };
    let (outputs, usage) = provider.respond_with_usage(req).await?;
    let call = outputs
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
    Ok((brief, usage))
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
        let (brief, _) = plan_ai(&fake, &json!({"statement": "Ship auth"}))
            .await
            .unwrap();
        assert_eq!(brief.goal, "Ship auth");
        assert_eq!(brief.in_scope, ["API"]);
    }

    #[tokio::test]
    async fn maps_full_plan_brief_and_passes_readiness() {
        let fake = Fake(Ok(vec![AiOutput::ToolCall(ToolCall {
            name: "build_plan_brief".into(),
            arguments: json!({
                "goal": "Ship auth",
                "in_scope": ["API"],
                "completion_criteria": ["Tests pass"],
                "daruma_target": "one plan",
                "why_now": "Customers need it",
                "decisions_made": ["Use OAuth"],
                "risks": ["Provider outage"],
                "constraints": ["No downtime"],
                "knowledge_base": ["Auth ADR"],
                "unverified_hypotheses": ["Existing tokens migrate"],
                "rejected_alternatives": ["Session cookies"],
                "out_of_scope": ["Billing"],
                "dependencies": ["Identity provider"],
                "required_artifacts": ["Migration guide"],
                "project_structure": "existing project"
            })
            .to_string(),
        })]));

        let (brief, _) = plan_ai(&fake, &json!({"statement": "Ship auth"}))
            .await
            .unwrap();

        assert_eq!(
            brief,
            PlanBrief {
                goal: "Ship auth".into(),
                in_scope: vec!["API".into()],
                completion_criteria: vec!["Tests pass".into()],
                daruma_target: "one plan".into(),
                why_now: Some("Customers need it".into()),
                decisions_made: vec!["Use OAuth".into()],
                risks: vec!["Provider outage".into()],
                constraints: vec!["No downtime".into()],
                knowledge_base: vec!["Auth ADR".into()],
                unverified_hypotheses: vec!["Existing tokens migrate".into()],
                rejected_alternatives: vec!["Session cookies".into()],
                out_of_scope: vec!["Billing".into()],
                dependencies: vec!["Identity provider".into()],
                required_artifacts: vec!["Migration guide".into()],
                project_structure: Some("existing project".into()),
            }
        );
        assert!(crate::check_readiness(&brief).is_ready);
    }

    #[tokio::test]
    async fn propagates_provider_error() {
        let error = plan_ai(&Fake(Err(AiError::new("boom"))), &json!({}))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("boom"));
    }
}
