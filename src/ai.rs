//! Planning's slice of the AI provider seam: re-exports the shared
//! [`layer_kit::ai`] infrastructure and adds the domain-specific tool
//! schemas `decompose`/`scope`/`analyze_complexity` need.

use serde_json::{json, Value};

pub use layer_kit::ai::{
    AiError, AiOutput, AiProvider, AiRequest, ToolCall, UNTRUSTED_CLOSE, UNTRUSTED_OPEN,
};
pub use layer_kit::ai::wrap_untrusted;

/// JSON schema for the `split_task` function tool used by `decompose`.
pub fn split_task_tool() -> Value {
    json!({
        "type": "function",
        "name": "split_task",
        "description": "Decompose a parent task into an ordered list of concrete sub-tasks.",
        "parameters": {
            "type": "object",
            "properties": {
                "subtasks": {
                    "type": "array",
                    "description": "Ordered sub-tasks the parent should be split into (at least 2).",
                    "minItems": 2,
                    "items": {
                        "type": "object",
                        "properties": {
                            "title": {
                                "type": "string",
                                "description": "Short, imperative sub-task title."
                            },
                            "description": {
                                "type": "string",
                                "description": "Optional detail or acceptance criteria."
                            }
                        },
                        "required": ["title"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["subtasks"],
            "additionalProperties": false
        }
    })
}

/// JSON schema for the `rescope_task` function tool used by `scope`. The
/// model returns the rewritten task body — the host turns it into
/// daruma's `Command::UpdateTask`.
pub fn rescope_task_tool() -> Value {
    json!({
        "type": "function",
        "name": "rescope_task",
        "description": "Rewrite a task's title and description at a target complexity. `up` broadens scope into an epic-style framing; `down` narrows it into a single concrete action.",
        "parameters": {
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "New short, imperative title (≤120 chars)."
                },
                "description": {
                    "type": "string",
                    "description": "New body — acceptance criteria, steps, context. May be empty."
                }
            },
            "required": ["title", "description"],
            "additionalProperties": false
        }
    })
}

/// JSON schema for the `report_complexity` function tool used by
/// `analyze_complexity`. The model scores every task in the batch; the
/// host turns the returned drafts into daruma's `ComplexityHint` rows and
/// upserts the `task_complexity_hints` projection.
pub fn report_complexity_tool() -> Value {
    json!({
        "type": "function",
        "name": "report_complexity",
        "description": "Report a complexity score for each task in the batch. \
                        Higher score => larger decomposition warranted.",
        "parameters": {
            "type": "object",
            "properties": {
                "hints": {
                    "type": "array",
                    "description": "One entry per input task, in the same order.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "task_id":              {"type": "string"},
                            "score":                {"type": "integer", "minimum": 1, "maximum": 10},
                            "recommended_subtasks": {"type": "integer", "minimum": 0, "maximum": 20},
                            "expansion_hint":       {"type": "string"},
                            "reasoning":            {"type": "string"}
                        },
                        "required": [
                            "task_id", "score", "recommended_subtasks",
                            "expansion_hint", "reasoning"
                        ],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["hints"],
            "additionalProperties": false
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_task_tool_shape() {
        let t = split_task_tool();
        assert_eq!(t["name"], "split_task");
        assert_eq!(t["parameters"]["required"][0], "subtasks");
    }

    #[test]
    fn rescope_task_tool_shape() {
        let t = rescope_task_tool();
        assert_eq!(t["name"], "rescope_task");
        assert_eq!(t["parameters"]["required"][0], "title");
        assert_eq!(t["parameters"]["required"][1], "description");
    }

    #[test]
    fn report_complexity_tool_shape() {
        let t = report_complexity_tool();
        assert_eq!(t["name"], "report_complexity");
        assert_eq!(t["parameters"]["required"][0], "hints");
        let req = t["parameters"]["properties"]["hints"]["items"]["required"]
            .as_array()
            .unwrap();
        let names: Vec<&str> = req.iter().filter_map(|v| v.as_str()).collect();
        for f in [
            "task_id",
            "score",
            "recommended_subtasks",
            "expansion_hint",
            "reasoning",
        ] {
            assert!(names.contains(&f), "schema missing field: {f}");
        }
    }
}
