use std::collections::{HashMap, HashSet};

use crate::blueprint::model::{DependencyStatus, NotReadyTodo, Readiness, Todo, TodoStatus};

pub fn validate_dependency_graph(todos: &[Todo]) -> anyhow::Result<()> {
    let flattened = flatten(todos);
    validate_todo_invariants(&flattened)?;
    let by_id = index_by_id(&flattened)?;
    for todo in &flattened {
        for dependency in &todo.depends_on {
            if dependency == &todo.id {
                anyhow::bail!("Todo {} cannot depend on itself", todo.id);
            }
            if !by_id.contains_key(dependency.as_str()) {
                anyhow::bail!("Todo {} depends on unknown Todo {dependency}", todo.id);
            }
        }
    }
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    for todo in &flattened {
        visit(todo.id.as_str(), &by_id, &mut visiting, &mut visited)?;
    }
    Ok(())
}

fn validate_todo_invariants(todos: &[&Todo]) -> anyhow::Result<()> {
    for todo in todos {
        if todo.created_by.as_deref().is_none_or(str::is_empty) {
            anyhow::bail!("Todo {} is missing Created By", todo.id);
        }
        match todo.status {
            TodoStatus::Pending => {}
            TodoStatus::InProgress if todo.owner.as_deref().is_none_or(str::is_empty) => {
                anyhow::bail!("in_progress Todo {} is missing Owner", todo.id)
            }
            TodoStatus::Completed => {
                if todo.completed_by.as_deref().is_none_or(str::is_empty) {
                    anyhow::bail!("completed Todo {} is missing Completed By", todo.id);
                }
                if todo.result_summary.as_deref().is_none_or(str::is_empty) {
                    anyhow::bail!("completed Todo {} is missing Result Summary", todo.id);
                }
                if todo.completion_criteria.iter().any(|item| !item.completed) {
                    anyhow::bail!(
                        "completed Todo {} has incomplete Completion Criteria",
                        todo.id
                    );
                }
                if todo.children.iter().any(|child| {
                    !matches!(child.status, TodoStatus::Completed | TodoStatus::Cancelled)
                }) {
                    anyhow::bail!("completed Todo {} has open child Todos", todo.id);
                }
            }
            TodoStatus::Blocked => {
                if todo.block_reason.as_deref().is_none_or(str::is_empty) {
                    anyhow::bail!("blocked Todo {} is missing Block Reason", todo.id);
                }
                if todo.handoff.is_empty() {
                    anyhow::bail!("blocked Todo {} is missing Handoff", todo.id);
                }
            }
            TodoStatus::Cancelled if todo.cancel_reason.as_deref().is_none_or(str::is_empty) => {
                anyhow::bail!("cancelled Todo {} is missing Cancel Reason", todo.id)
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn derive_readiness(todos: &[Todo]) -> anyhow::Result<Readiness> {
    validate_dependency_graph(todos)?;
    let flattened = flatten(todos);
    let by_id = index_by_id(&flattened)?;
    let mut readiness = Readiness::default();
    for todo in flattened {
        if todo.status != TodoStatus::Pending {
            continue;
        }
        let unsatisfied_dependencies = todo
            .depends_on
            .iter()
            .filter_map(|id| {
                let dependency = by_id[id.as_str()];
                (dependency.status != TodoStatus::Completed).then(|| DependencyStatus {
                    id: id.clone(),
                    status: dependency.status,
                })
            })
            .collect::<Vec<_>>();
        if unsatisfied_dependencies.is_empty() {
            readiness.ready.push(todo.id.clone());
        } else {
            readiness.not_ready.push(NotReadyTodo {
                id: todo.id.clone(),
                unsatisfied_dependencies,
            });
        }
    }
    Ok(readiness)
}

fn flatten(todos: &[Todo]) -> Vec<&Todo> {
    fn collect<'a>(todo: &'a Todo, output: &mut Vec<&'a Todo>) {
        output.push(todo);
        for child in &todo.children {
            collect(child, output);
        }
    }

    let mut output = Vec::new();
    for todo in todos {
        collect(todo, &mut output);
    }
    output
}

fn index_by_id<'a>(todos: &[&'a Todo]) -> anyhow::Result<HashMap<&'a str, &'a Todo>> {
    let mut indexed = HashMap::new();
    for todo in todos {
        if indexed.insert(todo.id.as_str(), *todo).is_some() {
            anyhow::bail!("duplicate Todo ID: {}", todo.id);
        }
    }
    Ok(indexed)
}

fn visit(
    id: &str,
    by_id: &HashMap<&str, &Todo>,
    visiting: &mut HashSet<String>,
    visited: &mut HashSet<String>,
) -> anyhow::Result<()> {
    if visited.contains(id) {
        return Ok(());
    }
    if !visiting.insert(id.to_string()) {
        anyhow::bail!("Todo dependency cycle includes {id}");
    }
    for dependency in &by_id[id].depends_on {
        visit(dependency, by_id, visiting, visited)?;
    }
    visiting.remove(id);
    visited.insert(id.to_string());
    Ok(())
}
