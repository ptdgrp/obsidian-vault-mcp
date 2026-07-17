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
use serde::Serialize;

use crate::{
    blueprint::{
        BlueprintCancelInput, BlueprintCloseInput, BlueprintCreateInput, BlueprintGetInput,
        BlueprintGetOutput, BlueprintIdInput, BlueprintListInput, BlueprintListOutput,
        BlueprintService, BlueprintStatus, BlueprintUpdateInput, CheckUpdate,
        CompletionCriterionInput, DodUpdateInput, StoredBlueprint, Todo, TodoAssignInput,
        TodoBlockInput, TodoCancelInput, TodoCompleteInput, TodoCreateInput, TodoInput,
        TodoListInput, TodoListOutput, TodoUpdateInput,
    },
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

fn json<T: Serialize + JsonSchema>(result: anyhow::Result<T>) -> Result<Json<T>, String> {
    let started = Instant::now();
    tracing::info!("blueprint.mcp.result.start");
    let result = result.map(Json).map_err(|error| error.to_string());
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
    #[tracing::instrument(name = "blueprint.mcp.tool", skip(self, r), fields(tool.name = "blueprint_create"))]
    fn blueprint_create(
        &self,
        Parameters(r): Parameters<BlueprintCreateInput>,
    ) -> Result<Json<crate::blueprint::BlueprintCreated>, String> {
        json(self.service.blueprint_create(r))
    }
    #[tool(
        description = "Read one Blueprint. The optional resume view is a focused recovery view."
    )]
    #[tracing::instrument(name = "blueprint.mcp.tool", skip(self, r), fields(tool.name = "blueprint_get"))]
    fn blueprint_get(
        &self,
        Parameters(r): Parameters<BlueprintGetInput>,
    ) -> Result<Json<BlueprintGetOutput>, String> {
        json(
            self.service
                .blueprint_view(&r.blueprint_id, r.view.as_deref()),
        )
    }
    #[tool(description = "List Blueprint IDs in one lifecycle state.")]
    #[tracing::instrument(name = "blueprint.mcp.tool", skip(self, r), fields(tool.name = "blueprint_list"))]
    fn blueprint_list(
        &self,
        Parameters(r): Parameters<BlueprintListInput>,
    ) -> Result<Json<BlueprintListOutput>, String> {
        json(
            self.service
                .blueprint_list_in(&r.state)
                .map(|blueprint_ids| BlueprintListOutput { blueprint_ids }),
        )
    }
    #[tool(
        description = "Update title, Intent, Constraints, Plan, Results, or Notes of an active Blueprint."
    )]
    #[tracing::instrument(name = "blueprint.mcp.tool", skip(self, r), fields(tool.name = "blueprint_update"))]
    fn blueprint_update(
        &self,
        Parameters(r): Parameters<BlueprintUpdateInput>,
    ) -> Result<Json<StoredBlueprint>, String> {
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
    #[tracing::instrument(name = "blueprint.mcp.tool", skip(self, r), fields(tool.name = "blueprint_status"))]
    fn blueprint_status(
        &self,
        Parameters(r): Parameters<BlueprintIdInput>,
    ) -> Result<Json<BlueprintStatus>, String> {
        json(self.service.blueprint_status(&r.blueprint_id))
    }
    #[tool(description = "Close an active Blueprint, recording incomplete work when applicable.")]
    #[tracing::instrument(name = "blueprint.mcp.tool", skip(self, r), fields(tool.name = "blueprint_close"))]
    fn blueprint_close(
        &self,
        Parameters(r): Parameters<BlueprintCloseInput>,
    ) -> Result<Json<StoredBlueprint>, String> {
        json(self.service.blueprint_close(
            &r.blueprint_id,
            &r.closed_by,
            r.reason.as_deref(),
            r.expected_etag.as_deref(),
        ))
    }
    #[tool(description = "Cancel an active Blueprint and record why its intent was abandoned.")]
    #[tracing::instrument(name = "blueprint.mcp.tool", skip(self, r), fields(tool.name = "blueprint_cancel"))]
    fn blueprint_cancel(
        &self,
        Parameters(r): Parameters<BlueprintCancelInput>,
    ) -> Result<Json<StoredBlueprint>, String> {
        json(self.service.blueprint_cancel(
            &r.blueprint_id,
            &r.cancelled_by,
            &r.reason,
            r.expected_etag.as_deref(),
        ))
    }
    #[tool(description = "Explicitly mark a Definition of Done item complete or incomplete.")]
    #[tracing::instrument(name = "blueprint.mcp.tool", skip(self, r), fields(tool.name = "dod_update"))]
    fn dod_update(
        &self,
        Parameters(r): Parameters<DodUpdateInput>,
    ) -> Result<Json<StoredBlueprint>, String> {
        json(self.service.dod_update_with_note(
            &r.blueprint_id,
            &r.dod_id,
            r.completed,
            r.note.as_deref(),
            r.expected_etag.as_deref(),
        ))
    }
    #[tool(description = "Create a root or child Todo in an active Blueprint.")]
    #[tracing::instrument(name = "blueprint.mcp.tool", skip(self, r), fields(tool.name = "todo_create"))]
    fn todo_create(
        &self,
        Parameters(r): Parameters<TodoCreateInput>,
    ) -> Result<Json<Todo>, String> {
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
    #[tracing::instrument(name = "blueprint.mcp.tool", skip(self, r), fields(tool.name = "todo_get"))]
    fn todo_get(&self, Parameters(r): Parameters<TodoInput>) -> Result<Json<Todo>, String> {
        json(self.service.todo_get(&r.blueprint_id, &r.todo_id))
    }
    #[tool(description = "List Todos with optional status, owner, and readiness filters.")]
    #[tracing::instrument(name = "blueprint.mcp.tool", skip(self, r), fields(tool.name = "todo_list"))]
    fn todo_list(
        &self,
        Parameters(r): Parameters<TodoListInput>,
    ) -> Result<Json<TodoListOutput>, String> {
        json(
            self.service
                .todo_list_filtered(&r.blueprint_id, r.status, r.owner.as_deref(), r.ready)
                .map(|todos| TodoListOutput { todos }),
        )
    }
    #[tool(description = "Update non-lifecycle Todo fields.")]
    #[tracing::instrument(name = "blueprint.mcp.tool", skip(self, r), fields(tool.name = "todo_update"))]
    fn todo_update(
        &self,
        Parameters(r): Parameters<TodoUpdateInput>,
    ) -> Result<Json<Todo>, String> {
        let criteria = r.completion_criteria.as_ref().map(|values| {
            values
                .iter()
                .map(|item: &CompletionCriterionInput| CheckUpdate {
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
    #[tracing::instrument(name = "blueprint.mcp.tool", skip(self, r), fields(tool.name = "todo_assign"))]
    fn todo_assign(
        &self,
        Parameters(r): Parameters<TodoAssignInput>,
    ) -> Result<Json<Todo>, String> {
        json(self.service.todo_assign(
            &r.blueprint_id,
            &r.todo_id,
            &r.owner,
            r.expected_etag.as_deref(),
        ))
    }
    #[tool(description = "Start a pending Todo once it has an owner and completed dependencies.")]
    #[tracing::instrument(name = "blueprint.mcp.tool", skip(self, r), fields(tool.name = "todo_start"))]
    fn todo_start(&self, Parameters(r): Parameters<TodoInput>) -> Result<Json<Todo>, String> {
        json(
            self.service
                .todo_start(&r.blueprint_id, &r.todo_id, r.expected_etag.as_deref()),
        )
    }
    #[tool(
        description = "Complete an in-progress Todo after its completion criteria and child work are done."
    )]
    #[tracing::instrument(name = "blueprint.mcp.tool", skip(self, r), fields(tool.name = "todo_complete"))]
    fn todo_complete(
        &self,
        Parameters(r): Parameters<TodoCompleteInput>,
    ) -> Result<Json<Todo>, String> {
        json(self.service.todo_complete(
            &r.blueprint_id,
            &r.todo_id,
            &r.completed_by,
            &r.summary,
            r.expected_etag.as_deref(),
        ))
    }
    #[tool(description = "Block an in-progress Todo with a reason and handoff.")]
    #[tracing::instrument(name = "blueprint.mcp.tool", skip(self, r), fields(tool.name = "todo_block"))]
    fn todo_block(&self, Parameters(r): Parameters<TodoBlockInput>) -> Result<Json<Todo>, String> {
        json(self.service.todo_block(
            &r.blueprint_id,
            &r.todo_id,
            &r.reason,
            &r.handoff,
            r.expected_etag.as_deref(),
        ))
    }
    #[tool(description = "Cancel a pending, in-progress, or blocked Todo with a reason.")]
    #[tracing::instrument(name = "blueprint.mcp.tool", skip(self, r), fields(tool.name = "todo_cancel"))]
    fn todo_cancel(
        &self,
        Parameters(r): Parameters<TodoCancelInput>,
    ) -> Result<Json<Todo>, String> {
        json(self.service.todo_cancel(
            &r.blueprint_id,
            &r.todo_id,
            &r.reason,
            r.expected_etag.as_deref(),
        ))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for BlueprintMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Blueprint planning protocol tools for the current Obsidian vault.")
    }
}
