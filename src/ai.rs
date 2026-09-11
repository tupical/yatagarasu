//! Planning's slice of the AI provider seam: re-exports the shared
//! [`layer_kit::ai`] infrastructure and adds the domain-specific tool
//! schemas `decompose` and `scope` need.

use serde_json::{json, Value};

pub use layer_kit::ai::wrap_untrusted;
pub use layer_kit::ai::{
    AiError, AiOutput, AiProvider, AiRequest, AiUsage, ToolCall, UNTRUSTED_CLOSE, UNTRUSTED_OPEN,
};

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
}
