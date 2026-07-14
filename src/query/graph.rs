use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::resolver::{RefResolver, ResolveResult};

use super::notes::note_title;
use super::{
    AuditAmbiguousLink, AuditLinkTotals, AuditLinksResult, AuditUnresolvedLink, NeighborhoodCenter,
    NeighborhoodDirection, NeighborhoodLink, NeighborhoodNote, NeighborhoodResult, Pagination,
    VaultQueries, reference_display,
};

impl VaultQueries {
    pub fn get_note_neighborhood(
        &self,
        target: &str,
        depth: usize,
        direction: NeighborhoodDirection,
    ) -> anyhow::Result<NeighborhoodResult> {
        if !(1..=3).contains(&depth) {
            anyhow::bail!("depth must be between 1 and 3");
        }
        let notes = self.index_notes()?;
        let (center_path, heading, block_id) = match RefResolver::resolve(target, &notes) {
            ResolveResult::Resolved {
                path,
                heading,
                block_id,
                ..
            } => (path, heading, block_id),
            ResolveResult::Ambiguous { candidates, .. } => anyhow::bail!(
                "ambiguous note reference: {}",
                candidates
                    .into_iter()
                    .map(|candidate| candidate.path)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            ResolveResult::Unresolved { reference } => {
                anyhow::bail!("unresolved note reference: {}", reference.raw)
            }
        };
        let by_path = notes
            .iter()
            .map(|note| (note.file.relative_path.as_str(), note))
            .collect::<BTreeMap<_, _>>();
        let mut edges = BTreeSet::new();
        for note in &notes {
            for link in &note.parsed.links {
                let reference = reference_display(&link.target, &link.reference);
                if let ResolveResult::Resolved { path, .. } =
                    RefResolver::resolve(&reference, &notes)
                {
                    edges.insert(NeighborhoodLink {
                        from: note.file.relative_path.clone(),
                        to: path,
                    });
                }
            }
        }
        let mut distance = BTreeMap::from([(center_path.clone(), 0usize)]);
        let mut queue = VecDeque::from([(center_path.clone(), 0usize)]);
        while let Some((path, current)) = queue.pop_front() {
            if current == depth {
                continue;
            }
            for edge in &edges {
                let next = match direction {
                    NeighborhoodDirection::Out if edge.from == path => Some(&edge.to),
                    NeighborhoodDirection::In if edge.to == path => Some(&edge.from),
                    NeighborhoodDirection::Both if edge.from == path => Some(&edge.to),
                    NeighborhoodDirection::Both if edge.to == path => Some(&edge.from),
                    _ => None,
                };
                if let Some(next) = next
                    && !distance.contains_key(next)
                {
                    distance.insert(next.clone(), current + 1);
                    queue.push_back((next.clone(), current + 1));
                }
            }
        }
        let mut candidates = distance
            .into_iter()
            .filter(|(path, _)| path != &center_path)
            .collect::<Vec<_>>();
        candidates.sort_by(|(left_path, left_distance), (right_path, right_distance)| {
            left_distance
                .cmp(right_distance)
                .then(natord::compare(left_path, right_path))
        });
        let omitted_notes = candidates.len().saturating_sub(50);
        candidates.truncate(50);
        let retained = candidates
            .iter()
            .map(|(path, _)| path.clone())
            .chain(std::iter::once(center_path.clone()))
            .collect::<BTreeSet<_>>();
        let mut links = edges
            .into_iter()
            .filter(|edge| retained.contains(&edge.from) && retained.contains(&edge.to))
            .collect::<Vec<_>>();
        let omitted_links = links.len().saturating_sub(100);
        links.truncate(100);
        let center_note = by_path
            .get(center_path.as_str())
            .expect("resolved center must be indexed");
        Ok(NeighborhoodResult {
            center: NeighborhoodCenter {
                path: center_path,
                title: Some(note_title(
                    &center_note.parsed,
                    &center_note.file.relative_path,
                )),
                heading,
                block_id,
            },
            notes: candidates
                .into_iter()
                .map(|(path, distance)| NeighborhoodNote {
                    title: by_path
                        .get(path.as_str())
                        .map(|note| note_title(&note.parsed, &note.file.relative_path)),
                    path,
                    distance,
                })
                .collect(),
            links,
            truncated: (omitted_notes > 0 || omitted_links > 0).then_some(true),
            omitted_notes: (omitted_notes > 0).then_some(omitted_notes),
            omitted_links: (omitted_links > 0).then_some(omitted_links),
        })
    }

    pub fn audit_links(&self, page: usize) -> anyhow::Result<AuditLinksResult> {
        if page == 0 {
            anyhow::bail!("page must be greater than or equal to 1");
        }
        let notes = self.index_notes()?;
        let mut unresolved = Vec::new();
        let mut ambiguous = Vec::new();
        for note in &notes {
            for link in &note.parsed.links {
                let source = super::link_location(&link.source.clone().into());
                let target = reference_display(&link.target, &link.reference);
                match RefResolver::resolve(&target, &notes) {
                    ResolveResult::Unresolved { .. } => {
                        unresolved.push(AuditUnresolvedLink { source, target })
                    }
                    ResolveResult::Ambiguous { candidates, .. } => {
                        let omitted_candidates = if candidates.len() > 20 {
                            Some(candidates.len() - 20)
                        } else {
                            None
                        };
                        ambiguous.push(AuditAmbiguousLink {
                            source,
                            target,
                            candidates: candidates
                                .into_iter()
                                .take(20)
                                .map(|candidate| candidate.path)
                                .collect(),
                            omitted_candidates,
                        });
                    }
                    ResolveResult::Resolved { .. } => {}
                }
            }
        }
        unresolved.sort_by(|left, right| natord::compare(&left.source, &right.source));
        ambiguous.sort_by(|left, right| natord::compare(&left.source, &right.source));
        let totals = AuditLinkTotals {
            unresolved: unresolved.len(),
            ambiguous: ambiguous.len(),
        };
        let start = page.saturating_sub(1).saturating_mul(50);
        let total_pages = totals
            .unresolved
            .div_ceil(50)
            .max(totals.ambiguous.div_ceil(50));
        Ok(AuditLinksResult {
            unresolved: unresolved.into_iter().skip(start).take(50).collect(),
            ambiguous: ambiguous.into_iter().skip(start).take(50).collect(),
            totals,
            pagination: Pagination { page, total_pages },
        })
    }
}
