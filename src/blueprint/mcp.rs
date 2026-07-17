use std::sync::Arc;
use std::time::Instant;

use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{
        router::tool::ToolRouter,
        wrapper::{Json, Parameters},
    },
    model::{ServerCapabilities, ServerInfo, Tool},
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    blueprint::{BlueprintCreateRequest, BlueprintService, CheckUpdate},
    vault::Vault,
};

pub async fn run_blueprint_mcp_server(vault: Vault) -> anyhow::Result<()> {
    let server = BlueprintMcp::new(vault)
        .serve((tokio::io::stdin(), tokio::io::stdout()))
        .await?;
    server.waiting().await?;
    Ok(())
}

#[derive(Clone)]
pub struct BlueprintMcp {
    service: Arc<BlueprintService>,
    tool_router: ToolRouter<Self>,
}

impl BlueprintMcp {
    pub fn new(vault: Vault) -> Self {
        Self {
            service: Arc::new(BlueprintService::new(vault.root)),
            tool_router: Self::tool_router(),
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn tool_definitions() -> Vec<Tool> {
        Self::tool_router().list_all()
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CreateRequest {
    title: String,
    created_by: String,
    intent: String,
    #[serde(default)]
    constraints: Vec<String>,
    definition_of_done: Vec<String>,
    plan: String,
}
#[derive(Debug, Deserialize, JsonSchema)]
struct IdRequest {
    blueprint_id: String,
}
#[derive(Debug, Deserialize, JsonSchema)]
struct GetRequest {
    blueprint_id: String,
    #[serde(default)]
    view: Option<String>,
}
#[derive(Debug, Deserialize, JsonSchema)]
struct ListRequest {
    #[serde(default = "active_state")]
    state: String,
}
#[derive(Debug, Deserialize, JsonSchema)]
struct BlueprintUpdateRequest {
    blueprint_id: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    intent: Option<String>,
    #[serde(default)]
    constraints: Option<Vec<String>>,
    #[serde(default)]
    plan: Option<String>,
    #[serde(default)]
    results: Option<String>,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    expected_etag: Option<String>,
}
#[derive(Debug, Deserialize, JsonSchema)]
struct CloseRequest {
    blueprint_id: String,
    closed_by: String,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    expected_etag: Option<String>,
}
#[derive(Debug, Deserialize, JsonSchema)]
struct CancelBlueprintRequest {
    blueprint_id: String,
    cancelled_by: String,
    reason: String,
    #[serde(default)]
    expected_etag: Option<String>,
}
#[derive(Debug, Deserialize, JsonSchema)]
struct DodUpdateRequest {
    blueprint_id: String,
    dod_id: String,
    completed: bool,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    expected_etag: Option<String>,
}
#[derive(Debug, Deserialize, JsonSchema)]
struct TodoCreateRequest {
    blueprint_id: String,
    title: String,
    created_by: String,
    #[serde(default)]
    parent_id: Option<String>,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    depends_on: Vec<String>,
    #[serde(default)]
    completion_criteria: Vec<String>,
    #[serde(default)]
    expected_etag: Option<String>,
}
#[derive(Debug, Deserialize, JsonSchema)]
struct TodoRequest {
    blueprint_id: String,
    todo_id: String,
    #[serde(default)]
    expected_etag: Option<String>,
}
#[derive(Debug, Deserialize, JsonSchema)]
struct TodoListRequest {
    blueprint_id: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    ready: Option<bool>,
}
#[derive(Debug, Deserialize, JsonSchema)]
struct CriterionRequest {
    text: String,
    #[serde(default)]
    completed: bool,
}
#[derive(Debug, Deserialize, JsonSchema)]
struct TodoUpdateRequest {
    blueprint_id: String,
    todo_id: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    depends_on: Option<Vec<String>>,
    #[serde(default)]
    completion_criteria: Option<Vec<CriterionRequest>>,
    #[serde(default)]
    handoff: Option<Vec<String>>,
    #[serde(default)]
    result_summary: Option<String>,
    #[serde(default)]
    expected_etag: Option<String>,
}
#[derive(Debug, Deserialize, JsonSchema)]
struct AssignRequest {
    blueprint_id: String,
    todo_id: String,
    owner: String,
    #[serde(default)]
    expected_etag: Option<String>,
}
#[derive(Debug, Deserialize, JsonSchema)]
struct CompleteRequest {
    blueprint_id: String,
    todo_id: String,
    completed_by: String,
    summary: String,
    #[serde(default)]
    expected_etag: Option<String>,
}
#[derive(Debug, Deserialize, JsonSchema)]
struct BlockRequest {
    blueprint_id: String,
    todo_id: String,
    reason: String,
    handoff: String,
    #[serde(default)]
    expected_etag: Option<String>,
}
#[derive(Debug, Deserialize, JsonSchema)]
struct CancelTodoRequest {
    blueprint_id: String,
    todo_id: String,
    reason: String,
    #[serde(default)]
    expected_etag: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct ToolResponse {
    /// The Blueprint protocol result for this tool invocation.
    data: serde_json::Value,
}

fn active_state() -> String {
    "active".to_string()
}
fn json<T: Serialize>(result: anyhow::Result<T>) -> Result<Json<ToolResponse>, String> {
    let started = Instant::now();
    tracing::info!("blueprint.mcp.result.start");
    let result = result
        .and_then(|value| serde_json::to_value(value).map_err(Into::into))
        .map(|data| Json(ToolResponse { data }))
        .map_err(|error| error.to_string());
    match &result {
        Ok(_) => tracing::info!(
            duration_ms = started.elapsed().as_millis() as u64,
            "blueprint.mcp.result.ok"
        ),
        Err(error) => {
            tracing::warn!(duration_ms = started.elapsed().as_millis() as u64, error = %error, "blueprint.mcp.result.error")
        }
    }
    result
}

#[tool_router]
impl BlueprintMcp {
    #[tool(description = "Create a Blueprint in this vault's automatic .blueprint workspace.")]
    fn blueprint_create(
        &self,
        Parameters(r): Parameters<CreateRequest>,
    ) -> Result<Json<ToolResponse>, String> {
        json(self.service.blueprint_create(BlueprintCreateRequest {
            title: r.title,
            created_by: r.created_by,
            intent: r.intent,
            constraints: r.constraints,
            definition_of_done: r.definition_of_done,
            plan: r.plan,
        }))
    }
    #[tool(
        description = "Read one Blueprint. The optional resume view is a focused recovery view."
    )]
    fn blueprint_get(
        &self,
        Parameters(r): Parameters<GetRequest>,
    ) -> Result<Json<ToolResponse>, String> {
        json(get_view(&self.service, &r.blueprint_id, r.view.as_deref()))
    }
    #[tool(description = "List Blueprint IDs in one lifecycle state.")]
    fn blueprint_list(
        &self,
        Parameters(r): Parameters<ListRequest>,
    ) -> Result<Json<ToolResponse>, String> {
        json(self.service.blueprint_list_in(&r.state))
    }
    #[tool(
        description = "Update title, Intent, Constraints, Plan, Results, or Notes of an active Blueprint."
    )]
    fn blueprint_update(
        &self,
        Parameters(r): Parameters<BlueprintUpdateRequest>,
    ) -> Result<Json<ToolResponse>, String> {
        json(self.service.blueprint_update(
            &r.blueprint_id,
            r.title.as_deref(),
            r.intent.as_deref(),
            r.constraints.as_deref(),
            r.plan.as_deref(),
            r.results.as_deref(),
            r.notes.as_deref(),
            r.expected_etag.as_deref(),
        ))
    }
    #[tool(description = "Return derived Todo readiness for one Blueprint.")]
    fn blueprint_status(
        &self,
        Parameters(r): Parameters<IdRequest>,
    ) -> Result<Json<ToolResponse>, String> {
        json(self.service.blueprint_status(&r.blueprint_id))
    }
    #[tool(description = "Close an active Blueprint, recording incomplete work when applicable.")]
    fn blueprint_close(
        &self,
        Parameters(r): Parameters<CloseRequest>,
    ) -> Result<Json<ToolResponse>, String> {
        json(self.service.blueprint_close(
            &r.blueprint_id,
            &r.closed_by,
            r.reason.as_deref(),
            r.expected_etag.as_deref(),
        ))
    }
    #[tool(description = "Cancel an active Blueprint and record why its intent was abandoned.")]
    fn blueprint_cancel(
        &self,
        Parameters(r): Parameters<CancelBlueprintRequest>,
    ) -> Result<Json<ToolResponse>, String> {
        json(self.service.blueprint_cancel(
            &r.blueprint_id,
            &r.cancelled_by,
            &r.reason,
            r.expected_etag.as_deref(),
        ))
    }
    #[tool(description = "Explicitly mark a Definition of Done item complete or incomplete.")]
    fn dod_update(
        &self,
        Parameters(r): Parameters<DodUpdateRequest>,
    ) -> Result<Json<ToolResponse>, String> {
        json(self.service.dod_update_with_note(
            &r.blueprint_id,
            &r.dod_id,
            r.completed,
            r.note.as_deref(),
            r.expected_etag.as_deref(),
        ))
    }
    #[tool(description = "Create a root or child Todo in an active Blueprint.")]
    fn todo_create(
        &self,
        Parameters(r): Parameters<TodoCreateRequest>,
    ) -> Result<Json<ToolResponse>, String> {
        json(self.service.todo_create_full(
            &r.blueprint_id,
            &r.title,
            &r.created_by,
            r.parent_id.as_deref(),
            r.owner.as_deref(),
            &r.depends_on,
            &r.completion_criteria,
            r.expected_etag.as_deref(),
        ))
    }
    #[tool(description = "Read one Todo.")]
    fn todo_get(
        &self,
        Parameters(r): Parameters<TodoRequest>,
    ) -> Result<Json<ToolResponse>, String> {
        json(self.service.todo_get(&r.blueprint_id, &r.todo_id))
    }
    #[tool(description = "List Todos with optional status, owner, and readiness filters.")]
    fn todo_list(
        &self,
        Parameters(r): Parameters<TodoListRequest>,
    ) -> Result<Json<ToolResponse>, String> {
        json(self.service.todo_list(&r.blueprint_id).and_then(|todos| {
            filter_todos(
                todos,
                r.status.as_deref(),
                r.owner.as_deref(),
                r.ready,
                &self.service,
                &r.blueprint_id,
            )
        }))
    }
    #[tool(description = "Update non-lifecycle Todo fields.")]
    fn todo_update(
        &self,
        Parameters(r): Parameters<TodoUpdateRequest>,
    ) -> Result<Json<ToolResponse>, String> {
        let criteria = r.completion_criteria.as_ref().map(|values| {
            values
                .iter()
                .map(|item| CheckUpdate {
                    text: item.text.clone(),
                    completed: item.completed,
                })
                .collect::<Vec<_>>()
        });
        json(self.service.todo_update(
            &r.blueprint_id,
            &r.todo_id,
            r.title.as_deref(),
            r.depends_on.as_deref(),
            criteria.as_deref(),
            r.handoff.as_deref(),
            r.result_summary.as_deref(),
            r.expected_etag.as_deref(),
        ))
    }
    #[tool(description = "Assign or reassign a Todo owner.")]
    fn todo_assign(
        &self,
        Parameters(r): Parameters<AssignRequest>,
    ) -> Result<Json<ToolResponse>, String> {
        json(self.service.todo_assign(
            &r.blueprint_id,
            &r.todo_id,
            &r.owner,
            r.expected_etag.as_deref(),
        ))
    }
    #[tool(description = "Start a pending Todo once it has an owner and completed dependencies.")]
    fn todo_start(
        &self,
        Parameters(r): Parameters<TodoRequest>,
    ) -> Result<Json<ToolResponse>, String> {
        json(
            self.service
                .todo_start(&r.blueprint_id, &r.todo_id, r.expected_etag.as_deref()),
        )
    }
    #[tool(
        description = "Complete an in-progress Todo after its completion criteria and child work are done."
    )]
    fn todo_complete(
        &self,
        Parameters(r): Parameters<CompleteRequest>,
    ) -> Result<Json<ToolResponse>, String> {
        json(self.service.todo_complete(
            &r.blueprint_id,
            &r.todo_id,
            &r.completed_by,
            &r.summary,
            r.expected_etag.as_deref(),
        ))
    }
    #[tool(description = "Block an in-progress Todo with a reason and handoff.")]
    fn todo_block(
        &self,
        Parameters(r): Parameters<BlockRequest>,
    ) -> Result<Json<ToolResponse>, String> {
        json(self.service.todo_block(
            &r.blueprint_id,
            &r.todo_id,
            &r.reason,
            &r.handoff,
            r.expected_etag.as_deref(),
        ))
    }
    #[tool(description = "Cancel a pending, in-progress, or blocked Todo with a reason.")]
    fn todo_cancel(
        &self,
        Parameters(r): Parameters<CancelTodoRequest>,
    ) -> Result<Json<ToolResponse>, String> {
        json(self.service.todo_cancel(
            &r.blueprint_id,
            &r.todo_id,
            &r.reason,
            r.expected_etag.as_deref(),
        ))
    }
}

fn get_view(
    service: &BlueprintService,
    blueprint_id: &str,
    view: Option<&str>,
) -> anyhow::Result<serde_json::Value> {
    let stored = service.blueprint_get(blueprint_id)?;
    match view.unwrap_or("full") {
        "full" => Ok(
            serde_json::json!({"id": stored.id, "state": stored.state, "etag": stored.etag, "source": stored.source}),
        ),
        "resume" => {
            let status = service.blueprint_status(blueprint_id)?;
            let active_todos = status
                .todos
                .iter()
                .filter(|todo| {
                    matches!(
                        todo.status,
                        crate::blueprint::model::TodoStatus::InProgress
                            | crate::blueprint::model::TodoStatus::Blocked
                    ) || status.ready_todos.iter().any(|id| id == &todo.id)
                })
                .collect::<Vec<_>>();
            Ok(serde_json::json!({
                "id": stored.id, "state": stored.state, "etag": stored.etag,
                "intent": section_body(&stored.source, "Intent"), "constraints": section_body(&stored.source, "Constraints"),
                "plan": section_body(&stored.source, "Plan"), "results": section_body(&stored.source, "Results"),
                "open_definition_of_done": status.open_definition_of_done,
                "active_todos": active_todos, "ready_todos": status.ready_todos, "not_ready_todos": status.not_ready_todos,
            }))
        }
        other => anyhow::bail!("view must be full or resume, got: {other}"),
    }
}

fn section_body(source: &str, section: &str) -> String {
    let heading = format!("## {section}");
    let Some(heading_start) = source
        .lines()
        .find(|line| **line == heading)
        .map(|line| line.as_ptr() as usize - source.as_ptr() as usize)
    else {
        return String::new();
    };
    let start = source[heading_start + heading.len()..]
        .find('\n')
        .map(|offset| heading_start + heading.len() + offset + 1)
        .unwrap_or(source.len());
    let end = source[start..]
        .find("\n## ")
        .map(|offset| start + offset)
        .unwrap_or(source.len());
    source[start..end].trim().to_string()
}

fn filter_todos(
    mut todos: Vec<crate::blueprint::model::Todo>,
    status: Option<&str>,
    owner: Option<&str>,
    ready: Option<bool>,
    service: &BlueprintService,
    blueprint_id: &str,
) -> anyhow::Result<Vec<crate::blueprint::model::Todo>> {
    let readiness = service.blueprint_status(blueprint_id)?.ready_todos;
    todos.retain(|todo| {
        status.is_none_or(|value| {
            serde_json::to_value(todo.status)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .as_deref()
                == Some(value)
        }) && owner.is_none_or(|value| todo.owner.as_deref() == Some(value))
            && ready.is_none_or(|value| readiness.iter().any(|id| id == &todo.id) == value)
    });
    Ok(todos)
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for BlueprintMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Blueprint planning protocol tools for the current Obsidian vault.")
    }
}
