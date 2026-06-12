use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::resolver::{RefResolver, ResolveResult};

use super::{
    AmbiguousLinksResult, GraphEdge, GraphNeighborhoodDirection, GraphNeighborhoodOptions,
    GraphNode, LinkEvidence, UnresolvedLinksResult, VaultGraphResult, VaultQueries, read_snippet,
};

impl VaultQueries {
    pub fn find_unresolved_links(&self) -> anyhow::Result<UnresolvedLinksResult> {
        let notes = self.index_notes()?;
        let mut links = Vec::new();
        for note in &notes {
            for link in &note.parsed.links {
                let resolved = RefResolver::resolve(&link.target, &notes);
                if matches!(resolved, ResolveResult::Unresolved { .. }) {
                    links.push(LinkEvidence {
                        source: link.source.clone().into(),
                        target: link.target.clone(),
                        alias: link.alias.clone(),
                        resolved: resolved.into(),
                        snippet: read_snippet(&note.file, &link.source),
                    });
                }
            }
        }
        links.sort_by(|a, b| {
            natord::compare(&a.source.path, &b.source.path)
                .then(a.source.line_start.cmp(&b.source.line_start))
        });
        let truncated = links.len() > self.vault.config.max_results;
        links.truncate(self.vault.config.max_results);
        Ok(UnresolvedLinksResult { links, truncated })
    }

    pub fn find_ambiguous_links(&self) -> anyhow::Result<AmbiguousLinksResult> {
        let notes = self.index_notes()?;
        let mut links = Vec::new();
        for note in &notes {
            for link in &note.parsed.links {
                let resolved = RefResolver::resolve(&link.target, &notes);
                if matches!(resolved, ResolveResult::Ambiguous { .. }) {
                    links.push(LinkEvidence {
                        source: link.source.clone().into(),
                        target: link.target.clone(),
                        alias: link.alias.clone(),
                        resolved: resolved.into(),
                        snippet: read_snippet(&note.file, &link.source),
                    });
                }
            }
        }
        links.sort_by(|a, b| {
            natord::compare(&a.source.path, &b.source.path)
                .then(a.source.line_start.cmp(&b.source.line_start))
        });
        let truncated = links.len() > self.vault.config.max_results;
        links.truncate(self.vault.config.max_results);
        Ok(AmbiguousLinksResult { links, truncated })
    }

    pub fn get_vault_graph(&self) -> anyhow::Result<VaultGraphResult> {
        let notes = self.index_notes()?;
        let nodes = graph_nodes(&notes);
        let mut edges = graph_edges(&notes);
        let truncated = edges.len() > self.vault.config.max_results;
        edges.truncate(self.vault.config.max_results);
        Ok(VaultGraphResult {
            nodes,
            edges,
            truncated,
        })
    }

    pub fn get_graph_neighborhood(
        &self,
        options: GraphNeighborhoodOptions,
    ) -> anyhow::Result<VaultGraphResult> {
        let notes = self.index_notes()?;
        let center = match RefResolver::resolve(&options.target, &notes) {
            ResolveResult::Resolved { path, .. } => path,
            ResolveResult::Ambiguous { candidates, .. } => {
                anyhow::bail!(
                    "graph neighborhood target is ambiguous: {} ({})",
                    options.target,
                    candidates
                        .iter()
                        .map(|candidate| candidate.path.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            ResolveResult::Unresolved { .. } => {
                anyhow::bail!(
                    "graph neighborhood target is unresolved: {}",
                    options.target
                )
            }
        };

        let nodes_by_path = graph_nodes(&notes)
            .into_iter()
            .map(|node| (node.path.clone(), node))
            .collect::<BTreeMap<_, _>>();
        let all_edges = graph_edges(&notes);
        let mut visited = BTreeSet::from([center.clone()]);
        let mut queue = VecDeque::from([(center, 0usize)]);

        while let Some((path, depth)) = queue.pop_front() {
            if depth >= options.depth {
                continue;
            }

            for edge in all_edges.iter().filter(|edge| edge.status == "resolved") {
                let next = match options.direction {
                    GraphNeighborhoodDirection::Out if edge.from == path => Some(edge.to.clone()),
                    GraphNeighborhoodDirection::In if edge.to == path => Some(edge.from.clone()),
                    GraphNeighborhoodDirection::Both if edge.from == path => Some(edge.to.clone()),
                    GraphNeighborhoodDirection::Both if edge.to == path => Some(edge.from.clone()),
                    _ => None,
                };
                if let Some(next) = next
                    && nodes_by_path.contains_key(&next)
                    && visited.insert(next.clone())
                {
                    queue.push_back((next, depth + 1));
                }
            }
        }

        let mut edges = all_edges
            .into_iter()
            .filter(|edge| edge_in_neighborhood(edge, &visited, options.include_unresolved))
            .collect::<Vec<_>>();
        edges.sort_by(|a, b| {
            natord::compare(&a.from, &b.from).then(natord::compare(&a.target, &b.target))
        });

        let truncated = edges.len() > self.vault.config.max_results;
        edges.truncate(self.vault.config.max_results);

        let nodes = visited
            .into_iter()
            .filter_map(|path| nodes_by_path.get(&path).cloned())
            .collect();

        Ok(VaultGraphResult {
            nodes,
            edges,
            truncated,
        })
    }
}

fn graph_nodes(notes: &[crate::resolver::IndexedNote]) -> Vec<GraphNode> {
    notes
        .iter()
        .map(|note| GraphNode {
            path: note.file.relative_path.clone(),
            title: note
                .parsed
                .headings
                .first()
                .map(|heading| heading.text.clone()),
            tags: note
                .parsed
                .tags
                .iter()
                .map(|tag| tag.tag.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
        })
        .collect()
}

fn graph_edges(notes: &[crate::resolver::IndexedNote]) -> Vec<GraphEdge> {
    let mut edges = Vec::new();
    for note in notes {
        for link in &note.parsed.links {
            let resolved = RefResolver::resolve(&link.target, notes);
            let (to, status) = match &resolved {
                ResolveResult::Resolved { path, .. } => (path.clone(), "resolved"),
                ResolveResult::Ambiguous { .. } => (link.target.clone(), "ambiguous"),
                ResolveResult::Unresolved { .. } => (link.target.clone(), "unresolved"),
            };
            edges.push(GraphEdge {
                source: link.source.clone().into(),
                from: note.file.relative_path.clone(),
                to,
                target: link.target.clone(),
                alias: link.alias.clone(),
                status: status.to_string(),
            });
        }
    }
    edges.sort_by(|a, b| {
        natord::compare(&a.from, &b.from).then(natord::compare(&a.target, &b.target))
    });
    edges
}

fn edge_in_neighborhood(
    edge: &GraphEdge,
    visited: &BTreeSet<String>,
    include_unresolved: bool,
) -> bool {
    if edge.status == "resolved" {
        visited.contains(&edge.from) && visited.contains(&edge.to)
    } else {
        include_unresolved && visited.contains(&edge.from)
    }
}
