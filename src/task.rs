//! Local task domain for the planning operations.
//!
//! The skeleton owns its own primitives (`TaskId`, `ProjectId`, `Priority`,
//! `Status`), the `Task` it reads (input to `scope`), and the
//! provider-neutral output drafts it emits — [`TaskDraft`] (one decomposed
//! sub-task) and [`TaskPatchDraft`] (a sparse rescope update). mcpbox maps
//! these onto taskagent's `NewTask` / `Task` / `TaskPatch` and wraps them in
//! the appropriate `Command` (`SplitTask` / `UpdateTask`) when dispatching.
//! Keeping these types local is what lets the layer compile with zero
//! taskagent dependency.

use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── IDs ─────────────────────────────────────────────────────────────────

/// Strongly-typed UUIDv7 identifier for a task.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskId(pub Uuid);

impl TaskId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "task_{}", self.0)
    }
}

impl std::str::FromStr for TaskId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.strip_prefix("task_").unwrap_or(s);
        Ok(Self(Uuid::parse_str(trimmed)?))
    }
}

/// Strongly-typed UUIDv7 identifier for a project.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectId(pub Uuid);

impl ProjectId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for ProjectId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ProjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "prj_{}", self.0)
    }
}

impl std::str::FromStr for ProjectId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.strip_prefix("prj_").unwrap_or(s);
        Ok(Self(Uuid::parse_str(trimmed)?))
    }
}

// ── Enums ───────────────────────────────────────────────────────────────

/// Task priority. Wire form: `p0`..`p3`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    P0,
    P1,
    #[default]
    P2,
    P3,
}

/// Task status. Wire form: snake_case.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Inbox,
    Todo,
    InProgress,
    InReview,
    Done,
    Cancelled,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Inbox => "inbox",
            Status::Todo => "todo",
            Status::InProgress => "in_progress",
            Status::InReview => "in_review",
            Status::Done => "done",
            Status::Cancelled => "cancelled",
        }
    }
}

// ── Task (input to `scope`) ─────────────────────────────────────────────

/// A task the planning layer reads (e.g. to rescope). Provider-neutral
/// mirror of taskagent's `Task`, carrying only the fields the planning
/// operations actually use. mcpbox maps its own `Task` onto this when it
/// invokes the layer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<ProjectId>,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub status: Status,
    #[serde(default)]
    pub priority: Priority,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

// ── Output drafts ───────────────────────────────────────────────────────

/// The structured result of decomposing a parent into one sub-task, before
/// it becomes a taskagent `NewTask`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskDraft {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
}

impl TaskDraft {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Default::default()
        }
    }
}

/// A sparse rescope update emitted by `scope` (`None` = leave unchanged).
/// mcpbox maps it onto taskagent's `TaskPatch` and wraps it in
/// `Command::UpdateTask { id, patch }`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskPatchDraft {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}
