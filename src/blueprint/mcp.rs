use std::sync::Arc;

use crate::{
    blueprint::{
        BlueprintCancelInput, BlueprintCloseInput, BlueprintCreateInput, BlueprintGetInput,
        BlueprintGetOutput, BlueprintIdInput, BlueprintListInput, BlueprintListOutput,
        BlueprintService, BlueprintStatus, BlueprintUpdateInput, CheckUpdate,
        CompletionCriterionInput, DodUpdateInput, EvidenceAddInput, RevisionAppendInput,
        StoredBlueprint, TodoAssignInput, TodoBlockInput, TodoCancelInput, TodoCompleteInput,
        TodoCreateInput, TodoCreateRequest, TodoInput, TodoListInput, TodoListOutput, TodoPatch,
        TodoUpdateInput, TodoView,
    },
    server::run_tool,
};
use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{
        router::tool::ToolRouter,
        wrapper::{Json, Parameters},
    },
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};

pub async fn run_blueprint_mcp_server(service: BlueprintService) -> anyhow::Result<()> {
    let server = BlueprintMcp::new(service)
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
    pub fn new(service: BlueprintService) -> Self {
        Self {
            service: Arc::new(service),
            tool_router: Self::tool_router(),
        }
    }

    pub fn tool_definitions() -> Vec<rmcp::model::Tool> {
        Self::tool_router().list_all()
    }
}

#[tool_router]
impl BlueprintMcp {
    #[tool(
        description = "Create a Blueprint in this vault's automatic .blueprint workspace. Rubric is required: the executing Agent reads and performs that evaluation procedure; this tool only stores it."
    )]
    fn blueprint_create(
        &self,
        Parameters(r): Parameters<BlueprintCreateInput>,
    ) -> Result<Json<crate::blueprint::BlueprintCreated>, String> {
        run_tool("blueprint_create", r, |r| self.service.blueprint_create(r))
    }
    #[tool(
        description = "Read one Blueprint. The optional resume view is a focused recovery view."
    )]
    fn blueprint_get(
        &self,
        Parameters(r): Parameters<BlueprintGetInput>,
    ) -> Result<Json<BlueprintGetOutput>, String> {
        run_tool("blueprint_get", r, |r| {
            self.service
                .blueprint_view(&r.blueprint_id, r.view.as_deref())
        })
    }
    #[tool(description = "List Blueprint IDs in one lifecycle state.")]
    fn blueprint_list(
        &self,
        Parameters(r): Parameters<BlueprintListInput>,
    ) -> Result<Json<BlueprintListOutput>, String> {
        run_tool("blueprint_list", r, |r| {
            self.service
                .blueprint_list(&r.state)
                .map(|blueprint_ids| BlueprintListOutput { blueprint_ids })
        })
    }
    #[tool(
        description = "Update title, Intent, Constraints, Plan, Rubric, Results, or Notes of an active Blueprint. When Rubric changes, the executing Agent owns and performs that evaluation procedure; this tool only stores it."
    )]
    fn blueprint_update(
        &self,
        Parameters(r): Parameters<BlueprintUpdateInput>,
    ) -> Result<Json<StoredBlueprint>, String> {
        run_tool("blueprint_update", r, |r| {
            self.service.blueprint_update_semantic(
                &r.blueprint_id,
                crate::blueprint::model::BlueprintPatch {
                    title: r.title,
                    intent: r.intent,
                    constraints: r.constraints,
                    plan: r.plan,
                    rubric: r.rubric,
                    results: r.results,
                    notes: r.notes,
                },
                r.changed_by.as_deref(),
                r.change_reason.as_deref(),
                r.expected_etag.as_deref(),
            )
        })
    }
    #[tool(description = "Return derived Todo readiness for one Blueprint.")]
    fn blueprint_status(
        &self,
        Parameters(r): Parameters<BlueprintIdInput>,
    ) -> Result<Json<BlueprintStatus>, String> {
        run_tool("blueprint_status", r, |r| {
            self.service.blueprint_status(&r.blueprint_id)
        })
    }
    #[tool(description = "Close an active Blueprint, recording incomplete work when applicable.")]
    fn blueprint_close(
        &self,
        Parameters(r): Parameters<BlueprintCloseInput>,
    ) -> Result<Json<StoredBlueprint>, String> {
        run_tool("blueprint_close", r, |r| {
            self.service.blueprint_close(
                &r.blueprint_id,
                &r.closed_by,
                r.reason.as_deref(),
                r.expected_etag.as_deref(),
            )
        })
    }
    #[tool(description = "Cancel an active Blueprint and record why its intent was abandoned.")]
    fn blueprint_cancel(
        &self,
        Parameters(r): Parameters<BlueprintCancelInput>,
    ) -> Result<Json<StoredBlueprint>, String> {
        run_tool("blueprint_cancel", r, |r| {
            self.service.blueprint_cancel(
                &r.blueprint_id,
                &r.cancelled_by,
                &r.reason,
                r.expected_etag.as_deref(),
            )
        })
    }
    #[tool(description = "Explicitly mark a Definition of Done item complete or incomplete.")]
    fn dod_update(
        &self,
        Parameters(r): Parameters<DodUpdateInput>,
    ) -> Result<Json<StoredBlueprint>, String> {
        run_tool("dod_update", r, |r| {
            self.service.dod_update(
                &r.blueprint_id,
                &r.dod_id,
                r.completed,
                r.note.as_deref(),
                r.expected_etag.as_deref(),
            )
        })
    }
    #[tool(description = "Create a root or child Todo in an active Blueprint.")]
    fn todo_create(
        &self,
        Parameters(r): Parameters<TodoCreateInput>,
    ) -> Result<Json<TodoView>, String> {
        run_tool("todo_create", r, |r| {
            self.service.todo_create(TodoCreateRequest {
                blueprint_id: r.blueprint_id,
                title: r.title,
                created_by: r.created_by,
                intent: r.intent,
                plan: r.plan,
                parent_id: r.parent_id,
                owner: r.owner,
                depends_on: r.depends_on,
                completion_criteria: r.completion_criteria,
                expected_blueprint_etag: r.expected_blueprint_etag.or(r.expected_etag),
            })
        })
    }
    #[tool(description = "Read one Todo.")]
    fn todo_get(&self, Parameters(r): Parameters<TodoInput>) -> Result<Json<TodoView>, String> {
        run_tool("todo_get", r, |r| {
            self.service.todo_get(&r.blueprint_id, &r.todo_id)
        })
    }
    #[tool(description = "List Todos with optional status, owner, and readiness filters.")]
    fn todo_list(
        &self,
        Parameters(r): Parameters<TodoListInput>,
    ) -> Result<Json<TodoListOutput>, String> {
        run_tool("todo_list", r, |r| {
            self.service
                .todo_list(&r.blueprint_id, r.status, r.owner.as_deref(), r.ready)
                .map(|todos| TodoListOutput { todos })
        })
    }
    #[tool(description = "Update non-lifecycle Todo fields.")]
    fn todo_update(
        &self,
        Parameters(r): Parameters<TodoUpdateInput>,
    ) -> Result<Json<TodoView>, String> {
        let criteria = r.completion_criteria.as_ref().map(|values| {
            values
                .iter()
                .map(|item: &CompletionCriterionInput| CheckUpdate {
                    text: item.text.clone(),
                    completed: item.completed,
                })
                .collect::<Vec<_>>()
        });
        run_tool("todo_update", r, |r| {
            self.service.todo_update(
                &r.blueprint_id,
                &r.todo_id,
                TodoPatch {
                    title: r.title,
                    depends_on: r.depends_on,
                    intent: r.intent,
                    completion_criteria: criteria,
                    plan: r.plan,
                    handoff: r.handoff.map(|items| {
                        items
                            .into_iter()
                            .map(|item| format!("- {}", item.trim()))
                            .collect::<Vec<_>>()
                            .join("\n")
                    }),
                    results: r.result_summary,
                    notes: r.notes,
                },
                crate::blueprint::TodoUpdateOptions {
                    changed_by: r.changed_by.as_deref(),
                    change_reason: r.change_reason.as_deref(),
                    expected_blueprint_etag: r
                        .expected_blueprint_etag
                        .as_deref()
                        .or(r.expected_etag.as_deref()),
                    expected_todo_etag: r.expected_todo_etag.as_deref(),
                },
            )
        })
    }
    #[tool(description = "Assign or reassign a Todo owner.")]
    fn todo_assign(
        &self,
        Parameters(r): Parameters<TodoAssignInput>,
    ) -> Result<Json<TodoView>, String> {
        run_tool("todo_assign", r, |r| {
            self.service.todo_assign(
                &r.blueprint_id,
                &r.todo_id,
                &r.owner,
                r.expected_etag.as_deref(),
            )
        })
    }
    #[tool(
        description = "Append globally unique Evidence to a Blueprint or Todo detail document. The service performs structural validation only (including references); the executing Agent judges semantic sufficiency through the Rubric."
    )]
    fn evidence_add(
        &self,
        Parameters(r): Parameters<EvidenceAddInput>,
    ) -> Result<Json<crate::blueprint::EvidenceItem>, String> {
        run_tool("evidence_add", r, |r| self.service.evidence_add(r))
    }
    #[tool(
        description = "Append an immutable, append-only fixed-field Revision History record. Existing Revision entries cannot be replaced or deleted."
    )]
    fn revision_append(
        &self,
        Parameters(r): Parameters<RevisionAppendInput>,
    ) -> Result<Json<crate::blueprint::RevisionEntry>, String> {
        run_tool("revision_append", r, |r| self.service.revision_append(r))
    }
    #[tool(description = "Start a pending Todo once it has an owner and completed dependencies.")]
    fn todo_start(&self, Parameters(r): Parameters<TodoInput>) -> Result<Json<TodoView>, String> {
        run_tool("todo_start", r, |r| {
            self.service
                .todo_start(&r.blueprint_id, &r.todo_id, r.expected_etag.as_deref())
        })
    }
    #[tool(
        description = "Complete an in-progress Todo after its completion criteria and child work are done."
    )]
    fn todo_complete(
        &self,
        Parameters(r): Parameters<TodoCompleteInput>,
    ) -> Result<Json<TodoView>, String> {
        run_tool("todo_complete", r, |r| {
            self.service.todo_complete(
                &r.blueprint_id,
                &r.todo_id,
                &r.completed_by,
                r.expected_blueprint_etag
                    .as_deref()
                    .or(r.expected_etag.as_deref()),
                r.expected_todo_etag.as_deref(),
            )
        })
    }
    #[tool(description = "Block an in-progress Todo with a reason and handoff.")]
    fn todo_block(
        &self,
        Parameters(r): Parameters<TodoBlockInput>,
    ) -> Result<Json<TodoView>, String> {
        run_tool("todo_block", r, |r| {
            self.service.todo_block(
                &r.blueprint_id,
                &r.todo_id,
                &r.reason,
                &r.handoff,
                r.expected_blueprint_etag
                    .as_deref()
                    .or(r.expected_etag.as_deref()),
                r.expected_todo_etag.as_deref(),
            )
        })
    }
    #[tool(description = "Cancel a pending, in-progress, or blocked Todo with a reason.")]
    fn todo_cancel(
        &self,
        Parameters(r): Parameters<TodoCancelInput>,
    ) -> Result<Json<TodoView>, String> {
        run_tool("todo_cancel", r, |r| {
            self.service.todo_cancel(
                &r.blueprint_id,
                &r.todo_id,
                &r.reason,
                r.expected_etag.as_deref(),
            )
        })
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for BlueprintMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Blueprint planning protocol tools for the current Obsidian vault.")
    }
}
