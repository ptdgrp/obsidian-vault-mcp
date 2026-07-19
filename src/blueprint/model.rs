use crate::query::McpNonNegativeInteger;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use std::str::FromStr;

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

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct DefinitionOfDoneItem {
    pub id: String,
    pub text: String,
    pub completed: bool,
}

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

#[derive(Clone, Debug, Serialize, JsonSchema, PartialEq, Eq)]
/// Central Todo Graph node; execution state is authoritative here, not in Todo detail files.
pub struct TodoGraphNode {
    pub id: String,
    pub title: String,
    #[serde(skip)]
    #[schemars(skip)]
    pub document: String,
    pub status: TodoStatus,
    #[serde(skip)]
    #[schemars(skip)]
    pub created_by: Option<String>,
    #[serde(skip)]
    #[schemars(skip)]
    pub owner: Option<String>,
    #[serde(skip)]
    #[schemars(skip)]
    pub completed_by: Option<String>,
    pub depends_on: Vec<String>,
    #[serde(skip)]
    #[schemars(skip)]
    pub block_reason: Option<String>,
    #[serde(skip)]
    #[schemars(skip)]
    pub cancel_reason: Option<String>,
    pub children: Vec<TodoGraphNode>,
}

#[derive(Clone, Debug, Serialize, JsonSchema, PartialEq, Eq)]
/// An Evidence block retained exactly as Markdown.
pub struct EvidenceItem {
    pub id: String,
    #[serde(skip)]
    #[schemars(skip)]
    pub markdown: String,
}

#[derive(Clone, Debug, Serialize, JsonSchema, PartialEq, Eq)]
/// An append-only revision record retained exactly as Markdown.
pub struct RevisionEntry {
    pub id: String,
    #[serde(skip)]
    #[schemars(skip)]
    pub markdown: String,
}

/// Submits one Evidence block to a Todo.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct EvidenceSubmitInput {
    pub blueprint_id: String,
    pub todo_id: String,
    pub changed_by: String,
    pub title: String,
    pub body: crate::blueprint::ExternalBody,
    #[serde(default)]
    pub expected_etag: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct EvidenceListInput {
    pub blueprint_id: String,
    pub todo_id: String,
    #[serde(default = "default_page")]
    pub page: usize,
}

#[derive(Clone, Debug, Serialize, JsonSchema, PartialEq, Eq)]
pub struct EvidenceSummary {
    pub id: String,
    pub title: String,
    pub todo_id: String,
    pub body_preview: String,
}

#[derive(Clone, Debug, Serialize, JsonSchema, PartialEq, Eq)]
pub struct EvidencePagination {
    #[schemars(with = "McpNonNegativeInteger")]
    pub page: usize,
    #[schemars(with = "McpNonNegativeInteger")]
    pub total_pages: usize,
    #[schemars(with = "McpNonNegativeInteger")]
    pub total_evidence: usize,
}

#[derive(Clone, Debug, Serialize, JsonSchema, PartialEq, Eq)]
pub struct EvidenceListOutput {
    pub evidence: Vec<EvidenceSummary>,
    pub pagination: EvidencePagination,
    pub index_status: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct ResultsInput {
    pub body: crate::blueprint::ExternalBody,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
}

fn default_page() -> usize {
    1
}

#[derive(Clone, Debug, Serialize, JsonSchema, PartialEq, Eq)]
/// Typed content of a standalone Todo detail document.
pub struct TodoDetail {
    pub id: String,
    pub blueprint_id: String,
    pub title: String,
    pub created_by: String,
    pub owner: String,
    pub completed_by: Option<String>,
    pub block_reason: Option<String>,
    pub cancel_reason: Option<String>,
    pub plan: String,
    pub completion_criteria: Vec<CheckItem>,
    pub handoff: String,
    pub result: String,
    pub evidence: Vec<EvidenceItem>,
    pub revisions: Vec<RevisionEntry>,
    pub notes: String,
}

/// A Todo aggregate: graph state comes from `blueprint.md`, content from its detail document.
#[derive(Clone, Debug, Serialize, JsonSchema, PartialEq, Eq)]
pub struct TodoView {
    pub graph: TodoGraphNode,
    pub detail: TodoDetail,
    pub blueprint_etag: String,
    pub todo_etag: String,
    pub ready: bool,
    pub unsatisfied_dependencies: Vec<DependencyStatus>,
    #[serde(skip)]
    projection: Todo,
}

impl Deref for TodoView {
    type Target = Todo;

    fn deref(&self) -> &Self::Target {
        &self.projection
    }
}

impl TodoView {
    pub(crate) fn new(
        graph: TodoGraphNode,
        detail: TodoDetail,
        blueprint_etag: String,
        todo_etag: String,
        ready: bool,
        unsatisfied_dependencies: Vec<DependencyStatus>,
        projection: Todo,
    ) -> Self {
        Self {
            graph,
            detail,
            blueprint_etag,
            todo_etag,
            ready,
            unsatisfied_dependencies,
            projection,
        }
    }
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

/// Creation request for a v2 Todo aggregate.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct TodoCreateRequest {
    pub blueprint_id: String,
    pub title: String,
    pub created_by: String,
    pub plan: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub completion_criteria: Vec<String>,
    #[serde(default)]
    pub expected_blueprint_etag: Option<String>,
}

/// A cross-document Todo update. Semantic fields require revision metadata.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct TodoPatch {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub depends_on: Option<Vec<String>>,
    #[serde(default)]
    pub completion_criteria: Option<Vec<CheckUpdate>>,
    #[serde(default)]
    pub plan: Option<String>,
    #[serde(default)]
    pub handoff: Option<String>,
    #[serde(default)]
    pub result: Option<ResultsInput>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct CheckUpdate {
    pub text: String,
    #[serde(default)]
    pub completed: bool,
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

/// The structured aggregate returned by `blueprint_get`.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct BlueprintGetOutput {
    pub id: String,
    pub state: BlueprintState,
    pub title: String,
    pub created_by: String,
    pub etag: String,
    pub intent: String,
    pub constraints: String,
    pub definition_of_done: Vec<DefinitionOfDoneItem>,
    pub plan: String,
    pub rubric: String,
    pub todos: Vec<TodoGraphNode>,
    pub results: String,
    pub revisions: Vec<RevisionEntry>,
    pub notes: String,
}

/// The concrete result of `blueprint_list`.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct BlueprintListOutput {
    pub blueprint_ids: Vec<String>,
}

/// The concrete result of `todo_list`.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct TodoListOutput {
    pub todos: Vec<TodoView>,
}

/// Input accepted by `blueprint_create`.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct BlueprintCreateInput {
    pub title: String,
    pub created_by: String,
    pub intent: crate::blueprint::ExternalBody,
    #[serde(default)]
    pub constraints: Vec<String>,
    pub definition_of_done: Vec<String>,
    pub plan: crate::blueprint::ExternalBody,
    /// The evaluation procedure selected for this Blueprint.
    #[serde(default)]
    pub rubric: Option<crate::blueprint::ExternalBody>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct BlueprintGetInput {
    pub blueprint_id: String,
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
    pub intent: Option<crate::blueprint::ExternalBody>,
    #[serde(default)]
    pub constraints: Option<Vec<String>>,
    #[serde(default)]
    pub plan: Option<crate::blueprint::ExternalBody>,
    #[serde(default)]
    pub rubric: Option<crate::blueprint::ExternalBody>,
    #[serde(default)]
    pub changed_by: Option<String>,
    #[serde(default)]
    pub change_reason: Option<String>,
    #[serde(default)]
    pub results: Option<ResultsInput>,
    #[serde(default)]
    pub notes: Option<crate::blueprint::ExternalBody>,
    #[serde(default)]
    pub expected_etag: Option<String>,
}

/// Patchable Blueprint sections. Changes to semantic fields require a revision record.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct BlueprintPatch {
    #[serde(default)]
    pub intent: Option<String>,
    #[serde(default)]
    pub constraints: Option<Vec<String>>,
    #[serde(default)]
    pub plan: Option<String>,
    #[serde(default)]
    pub rubric: Option<String>,
    #[serde(default)]
    pub results: Option<ResultsInput>,
    #[serde(default)]
    pub notes: Option<String>,
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
    pub plan: crate::blueprint::ExternalBody,
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
    #[serde(default)]
    pub expected_blueprint_etag: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct TodoInput {
    pub blueprint_id: String,
    pub todo_id: String,
    #[serde(default)]
    pub expected_etag: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct TodoStartInput {
    pub blueprint_id: String,
    pub todo_id: String,
    pub changed_by: String,
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
    pub plan: Option<crate::blueprint::ExternalBody>,
    #[serde(default)]
    pub result: Option<ResultsInput>,
    #[serde(default)]
    pub notes: Option<crate::blueprint::ExternalBody>,
    pub changed_by: String,
    #[serde(default)]
    pub change_reason: Option<String>,
    #[serde(default)]
    pub expected_etag: Option<String>,
    #[serde(default)]
    pub expected_blueprint_etag: Option<String>,
    #[serde(default)]
    pub expected_todo_etag: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct TodoAssignInput {
    pub blueprint_id: String,
    pub todo_id: String,
    pub owner: String,
    pub changed_by: String,
    #[serde(default)]
    pub expected_etag: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct TodoCompleteInput {
    pub blueprint_id: String,
    pub todo_id: String,
    pub changed_by: String,
    #[serde(default)]
    pub expected_etag: Option<String>,
    #[serde(default)]
    pub expected_blueprint_etag: Option<String>,
    #[serde(default)]
    pub expected_todo_etag: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct TodoBlockInput {
    pub blueprint_id: String,
    pub todo_id: String,
    pub reason: String,
    pub handoff: String,
    pub changed_by: String,
    #[serde(default)]
    pub expected_etag: Option<String>,
    #[serde(default)]
    pub expected_blueprint_etag: Option<String>,
    #[serde(default)]
    pub expected_todo_etag: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct TodoCancelInput {
    pub blueprint_id: String,
    pub todo_id: String,
    pub reason: String,
    pub changed_by: String,
    #[serde(default)]
    pub expected_etag: Option<String>,
}

fn active_state() -> String {
    "active".to_string()
}
