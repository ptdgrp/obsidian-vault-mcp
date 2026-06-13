use std::fs;

use globset::{Glob, GlobSetBuilder};
use rayon::prelude::*;
use regex::{Regex, RegexBuilder};

use crate::parser::{slice_text, source_for_line};
use crate::vault::NoteFile;

use super::{SearchRegexResult, SearchTextResult, TextMatch, VaultQueries};

const MAX_SEARCH_SNIPPET_CHARS: usize = 240;

#[derive(Debug)]
struct RawTextMatch {
    file: NoteFile,
    line_no: u64,
    total_lines: u64,
}

#[derive(Debug)]
struct RawFileMatches {
    file: NoteFile,
    lines: Vec<(u64, u64)>,
}

impl VaultQueries {
    pub fn search_text(
        &self,
        query: &str,
        case_sensitive: bool,
        context_lines: usize,
    ) -> anyhow::Result<SearchTextResult> {
        let needle = if case_sensitive {
            query.to_string()
        } else {
            query.to_lowercase()
        };
        let raw_matches = self.collect_text_matches(|line| {
            if case_sensitive {
                line.contains(&needle)
            } else {
                line.to_lowercase().contains(&needle)
            }
        })?;
        let (matches, truncated) = self.materialize_search_matches(
            raw_matches,
            context_lines,
            self.vault.config.max_results,
        )?;
        Ok(SearchTextResult {
            query: query.to_string(),
            matches,
            truncated,
        })
    }

    pub fn search_regex(
        &self,
        pattern: &str,
        case_sensitive: bool,
        context_lines: usize,
        path_glob: Option<&str>,
    ) -> anyhow::Result<SearchRegexResult> {
        let regex = RegexBuilder::new(pattern)
            .case_insensitive(!case_sensitive)
            .build()?;
        let path_filter = match path_glob {
            Some(pattern) => {
                let mut builder = GlobSetBuilder::new();
                builder.add(Glob::new(pattern)?);
                Some(builder.build()?)
            }
            None => None,
        };

        let raw_matches = self.collect_regex_matches(&regex, path_filter.as_ref())?;
        let (matches, truncated) = self.materialize_search_matches(
            raw_matches,
            context_lines,
            self.vault.config.max_results,
        )?;

        Ok(SearchRegexResult {
            pattern: pattern.to_string(),
            path_glob: path_glob.map(ToOwned::to_owned),
            matches,
            truncated,
        })
    }

    fn collect_text_matches(
        &self,
        matches_line: impl Fn(&str) -> bool + Sync,
    ) -> anyhow::Result<Vec<RawTextMatch>> {
        let mut matches: Vec<RawTextMatch> = self
            .vault
            .list_notes()?
            .into_par_iter()
            .map(|file| collect_matches_in_file(file, &matches_line))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();
        sort_raw_matches(&mut matches);
        Ok(matches)
    }

    fn collect_regex_matches(
        &self,
        regex: &Regex,
        path_filter: Option<&globset::GlobSet>,
    ) -> anyhow::Result<Vec<RawTextMatch>> {
        let mut matches: Vec<RawTextMatch> = self
            .vault
            .list_notes()?
            .into_par_iter()
            .filter(|file| {
                path_filter
                    .as_ref()
                    .is_none_or(|filter| filter.is_match(&file.relative_path))
            })
            .map(|file| collect_matches_in_file(file, |line| regex.is_match(line)))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();
        sort_raw_matches(&mut matches);
        Ok(matches)
    }

    fn materialize_search_matches(
        &self,
        raw_matches: Vec<RawTextMatch>,
        context_lines: usize,
        max_results: usize,
    ) -> anyhow::Result<(Vec<TextMatch>, bool)> {
        let truncated = raw_matches.len() > max_results;
        let selected: Vec<RawTextMatch> = raw_matches.into_iter().take(max_results).collect();
        let mut matches: Vec<TextMatch> = group_matches_by_file(selected)
            .into_par_iter()
            .map(|raw| materialize_file_matches(self, raw, context_lines))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();
        matches.sort_by(|a, b| {
            natord::compare(&a.source.path, &b.source.path)
                .then(a.source.line_start.cmp(&b.source.line_start))
                .then(a.source.line_end.cmp(&b.source.line_end))
        });
        Ok((matches, truncated))
    }
}

fn collect_matches_in_file(
    file: NoteFile,
    matches_line: impl Fn(&str) -> bool,
) -> anyhow::Result<Vec<RawTextMatch>> {
    let content = fs::read_to_string(&file.path)?;
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len() as u64;
    Ok(lines
        .iter()
        .enumerate()
        .filter_map(|(zero_idx, line)| {
            matches_line(line).then_some(RawTextMatch {
                file: file.clone(),
                line_no: zero_idx as u64 + 1,
                total_lines,
            })
        })
        .collect())
}

fn materialize_file_matches(
    queries: &VaultQueries,
    raw: RawFileMatches,
    context_lines: usize,
) -> anyhow::Result<Vec<TextMatch>> {
    let content = fs::read_to_string(&raw.file.path)?;
    let parsed = queries.parse_file_cached(&raw.file.path, raw.file.relative_path.clone())?;
    Ok(raw
        .lines
        .into_iter()
        .map(|(line_no, total_lines)| {
            let start = line_no.saturating_sub(context_lines as u64).max(1);
            let end = (line_no + context_lines as u64).min(total_lines);
            let source = source_for_line(&raw.file.relative_path, &content, &parsed, start, end);
            let snippet = slice_text(&content, source.byte_start, source.byte_end);
            TextMatch {
                source: source.into(),
                snippet: truncate_search_snippet(&snippet),
            }
        })
        .collect())
}

fn sort_raw_matches(matches: &mut [RawTextMatch]) {
    matches.sort_by(|a, b| {
        natord::compare(&a.file.relative_path, &b.file.relative_path)
            .then(a.line_no.cmp(&b.line_no))
    });
}

fn group_matches_by_file(matches: Vec<RawTextMatch>) -> Vec<RawFileMatches> {
    let mut groups: Vec<RawFileMatches> = Vec::new();
    for raw in matches {
        if let Some(last) = groups
            .last_mut()
            .filter(|group| group.file.relative_path == raw.file.relative_path)
        {
            last.lines.push((raw.line_no, raw.total_lines));
        } else {
            groups.push(RawFileMatches {
                file: raw.file,
                lines: vec![(raw.line_no, raw.total_lines)],
            });
        }
    }
    groups
}

fn truncate_search_snippet(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars().take(MAX_SEARCH_SNIPPET_CHARS) {
        out.push(ch);
    }
    if input.chars().count() > MAX_SEARCH_SNIPPET_CHARS {
        out.push_str("...");
    }
    out
}
