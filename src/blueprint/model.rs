use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    Blocked,
    Cancelled,
}

impl TodoStatus {
    pub const fn marker(self) -> &'static str {
        match self {
            Self::Pending => " ",
            Self::InProgress => "/",
            Self::Completed => "x",
            Self::Blocked => "?",
            Self::Cancelled => "-",
        }
    }
}

impl TodoStatus {
    pub fn from_marker(marker: char) -> Option<Self> {
        match marker {
            ' ' => Some(Self::Pending),
            '/' => Some(Self::InProgress),
            'x' | 'X' => Some(Self::Completed),
            '?' => Some(Self::Blocked),
            '-' => Some(Self::Cancelled),
            _ => None,
        }
    }
}

impl TryFrom<&str> for TodoStatus {
    type Error = anyhow::Error;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "pending" => Ok(TodoStatus::Pending),
            "in_progress" => Ok(TodoStatus::InProgress),
            "completed" => Ok(TodoStatus::Completed),
            "blocked" => Ok(TodoStatus::Blocked),
            "cancelled" => Ok(TodoStatus::Cancelled),
            _ => {
                anyhow::bail!(
                    "invalid status: '{value}'; expected one of: pending, in_progress, completed, blocked, or cancelled"
                )
            }
        }
    }
}

impl FromStr for TodoStatus {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct CheckItem {
    pub text: String,
    pub completed: bool,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// Lifecycle state recorded in Blueprint v2 frontmatter.
pub enum BlueprintState {
    Active,
    Closed,
    Cancelled,
}

impl TryFrom<&str> for BlueprintState {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "active" => Ok(Self::Active),
            "closed" => Ok(Self::Closed),
            "cancelled" => Ok(Self::Cancelled),
            _ => anyhow::bail!(
                "invalid Blueprint state: '{value}'; expected active, closed, or cancelled"
            ),
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug, Serialize, JsonSchema, PartialEq, Eq)]
/// Central Todo Graph node; execution state is authoritative here, not in Todo detail files.
pub struct TodoGraphNode {
    pub id: String,
    pub title: String,
    pub document: String,
    pub status: TodoStatus,
    pub created_by: Option<String>,
    pub owner: Option<String>,
    pub completed_by: Option<String>,
    pub depends_on: Vec<String>,
    pub block_reason: Option<String>,
    pub cancel_reason: Option<String>,
    pub children: Vec<TodoGraphNode>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Serialize, JsonSchema, PartialEq, Eq)]
/// An Evidence block retained exactly as Markdown.
pub struct EvidenceItem {
    pub id: String,
    pub markdown: String,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Serialize, JsonSchema, PartialEq, Eq)]
/// An append-only revision record retained exactly as Markdown.
pub struct RevisionEntry {
    pub id: String,
    pub markdown: String,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Serialize, JsonSchema, PartialEq, Eq)]
/// Typed content of a standalone Todo detail document.
pub struct TodoDetail {
    pub id: String,
    pub blueprint_id: String,
    pub title: String,
    pub intent: String,
    pub completion_criteria: Vec<CheckItem>,
    pub plan: String,
    pub handoff: String,
    pub results: String,
    pub evidence: Vec<EvidenceItem>,
    pub revisions: Vec<RevisionEntry>,
    pub notes: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct Todo {
    pub id: String,
    pub title: String,
    pub status: TodoStatus,
    pub created_by: Option<String>,
    pub owner: Option<String>,
    pub completed_by: Option<String>,
    pub depends_on: Vec<String>,
    pub completion_criteria: Vec<CheckItem>,
    pub handoff: Vec<String>,
    pub result_summary: Option<String>,
    pub references: Vec<String>,
    pub block_reason: Option<String>,
    pub cancel_reason: Option<String>,
    pub children: Vec<Todo>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct DependencyStatus {
    pub id: String,
    pub status: TodoStatus,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct NotReadyTodo {
    pub id: String,
    pub unsatisfied_dependencies: Vec<DependencyStatus>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct Readiness {
    pub ready: Vec<String>,
    pub not_ready: Vec<NotReadyTodo>,
}

/// The concrete result of `blueprint_get` for either supported view.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct BlueprintGetOutput {
    pub id: String,
    pub state: String,
    pub etag: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume: Option<BlueprintResumeOutput>,
}

/// The focused recovery data returned by `blueprint_get` with `view: "resume"`.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct BlueprintResumeOutput {
    pub intent: String,
    pub constraints: String,
    pub plan: String,
    pub results: String,
    pub open_definition_of_done: Vec<String>,
    pub active_todos: Vec<Todo>,
    pub ready_todos: Vec<String>,
    pub not_ready_todos: Vec<NotReadyTodo>,
}

/// The concrete result of `blueprint_list`.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct BlueprintListOutput {
    pub blueprint_ids: Vec<String>,
}

/// The concrete result of `todo_list`.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct TodoListOutput {
    pub todos: Vec<Todo>,
}

/// Input accepted by `blueprint_create`.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct BlueprintCreateInput {
    pub title: String,
    pub created_by: String,
    pub intent: String,
    #[serde(default)]
    pub constraints: Vec<String>,
    pub definition_of_done: Vec<String>,
    pub plan: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct BlueprintGetInput {
    pub blueprint_id: String,
    #[serde(default)]
    pub view: Option<String>,
}
/// Identifies a Blueprint for operations that do not need a view selector.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct BlueprintIdInput {
    pub blueprint_id: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct BlueprintListInput {
    #[serde(default = "active_state")]
    pub state: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct BlueprintUpdateInput {
    pub blueprint_id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub intent: Option<String>,
    #[serde(default)]
    pub constraints: Option<Vec<String>>,
    #[serde(default)]
    pub plan: Option<String>,
    #[serde(default)]
    pub results: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub expected_etag: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct BlueprintCloseInput {
    pub blueprint_id: String,
    pub closed_by: String,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub expected_etag: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct BlueprintCancelInput {
    pub blueprint_id: String,
    pub cancelled_by: String,
    pub reason: String,
    #[serde(default)]
    pub expected_etag: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct DodUpdateInput {
    pub blueprint_id: String,
    pub dod_id: String,
    pub completed: bool,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub expected_etag: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct TodoCreateInput {
    pub blueprint_id: String,
    pub title: String,
    pub created_by: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub completion_criteria: Vec<String>,
    #[serde(default)]
    pub expected_etag: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct TodoInput {
    pub blueprint_id: String,
    pub todo_id: String,
    #[serde(default)]
    pub expected_etag: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct TodoListInput {
    pub blueprint_id: String,
    #[serde(default)]
    pub status: Option<TodoStatus>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub ready: Option<bool>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct CompletionCriterionInput {
    pub text: String,
    #[serde(default)]
    pub completed: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct TodoUpdateInput {
    pub blueprint_id: String,
    pub todo_id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub depends_on: Option<Vec<String>>,
    #[serde(default)]
    pub completion_criteria: Option<Vec<CompletionCriterionInput>>,
    #[serde(default)]
    pub handoff: Option<Vec<String>>,
    #[serde(default)]
    pub result_summary: Option<String>,
    #[serde(default)]
    pub expected_etag: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct TodoAssignInput {
    pub blueprint_id: String,
    pub todo_id: String,
    pub owner: String,
    #[serde(default)]
    pub expected_etag: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct TodoCompleteInput {
    pub blueprint_id: String,
    pub todo_id: String,
    pub completed_by: String,
    pub summary: String,
    #[serde(default)]
    pub expected_etag: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct TodoBlockInput {
    pub blueprint_id: String,
    pub todo_id: String,
    pub reason: String,
    pub handoff: String,
    #[serde(default)]
    pub expected_etag: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct TodoCancelInput {
    pub blueprint_id: String,
    pub todo_id: String,
    pub reason: String,
    #[serde(default)]
    pub expected_etag: Option<String>,
}

fn active_state() -> String {
    "active".to_string()
}
