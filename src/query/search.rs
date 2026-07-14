use std::fs;

use rayon::prelude::*;
use regex::RegexBuilder;

use crate::vault::NoteFile;

use super::path_filter::PathFilter;
use super::public::{Locator, PageSlice};
use super::{SearchPagination, SearchRegexResult, SearchTextResult, TextMatch, VaultQueries};

const SEARCH_PAGE_SIZE: usize = 50;
const MAX_SEARCH_PREVIEW_CHARS: usize = 240;
const ELLIPSIS: &str = "...";

#[derive(Debug)]
struct RawTextMatch {
    file: NoteFile,
    line_no: u64,
    line: String,
    match_start: usize,
    match_end: usize,
}

impl VaultQueries {
    pub fn search_text(
        &self,
        query: &str,
        case_sensitive: bool,
        include: &[String],
        exclude: &[String],
        page: usize,
    ) -> anyhow::Result<SearchTextResult> {
        let path_filter = PathFilter::new(include, exclude)?;
        let raw_matches = if query.is_empty() {
            self.collect_text_matches(&path_filter, |_| Some(0..0))?
        } else {
            let regex = RegexBuilder::new(&regex::escape(query))
                .case_insensitive(!case_sensitive)
                .build()?;
            self.collect_text_matches(&path_filter, |line| regex.find(line).map(|m| m.range()))?
        };
        let (matches, pagination) = materialize_search_page(raw_matches, page)?;
        Ok(SearchTextResult {
            matches,
            pagination,
        })
    }

    pub fn search_regex(
        &self,
        pattern: &str,
        case_sensitive: bool,
        include: &[String],
        exclude: &[String],
        page: usize,
    ) -> anyhow::Result<SearchRegexResult> {
        let path_filter = PathFilter::new(include, exclude)?;
        let regex = RegexBuilder::new(pattern)
            .case_insensitive(!case_sensitive)
            .build()?;

        let raw_matches =
            self.collect_text_matches(&path_filter, |line| regex.find(line).map(|m| m.range()))?;
        let (matches, pagination) = materialize_search_page(raw_matches, page)?;

        Ok(SearchRegexResult {
            matches,
            pagination,
        })
    }

    fn collect_text_matches(
        &self,
        path_filter: &PathFilter,
        first_match: impl Fn(&str) -> Option<std::ops::Range<usize>> + Sync,
    ) -> anyhow::Result<Vec<RawTextMatch>> {
        let mut matches: Vec<RawTextMatch> = self
            .vault
            .list_notes()?
            .into_par_iter()
            .filter(|file| path_filter.is_match(&file.relative_path))
            .map(|file| collect_matches_in_file(file, &first_match))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();
        sort_raw_matches(&mut matches);
        Ok(matches)
    }
}

fn collect_matches_in_file(
    file: NoteFile,
    first_match: impl Fn(&str) -> Option<std::ops::Range<usize>>,
) -> anyhow::Result<Vec<RawTextMatch>> {
    let content = fs::read_to_string(&file.path)?;
    Ok(content
        .lines()
        .enumerate()
        .filter_map(|(zero_idx, line)| {
            first_match(line).map(|range| RawTextMatch {
                file: file.clone(),
                line_no: zero_idx as u64 + 1,
                line: line.to_string(),
                match_start: range.start,
                match_end: range.end,
            })
        })
        .collect())
}

fn materialize_search_page(
    raw_matches: Vec<RawTextMatch>,
    page: usize,
) -> anyhow::Result<(Vec<TextMatch>, SearchPagination)> {
    let slice = PageSlice::new(raw_matches, page, SEARCH_PAGE_SIZE)?;
    let total_matches = slice.total_items();
    let pagination = slice.pagination();
    let matches = slice
        .into_items()
        .into_iter()
        .map(|raw| TextMatch {
            source: Locator::lines(&raw.file.relative_path, raw.line_no, raw.line_no),
            preview: centered_preview(&raw.line, raw.match_start, raw.match_end),
        })
        .collect();
    Ok((
        matches,
        SearchPagination {
            page: pagination.page,
            total_pages: pagination.total_pages,
            total_matches,
        },
    ))
}

fn sort_raw_matches(matches: &mut [RawTextMatch]) {
    matches.sort_by(|a, b| {
        natord::compare(&a.file.relative_path, &b.file.relative_path)
            .then(a.line_no.cmp(&b.line_no))
    });
}

fn centered_preview(line: &str, match_start: usize, match_end: usize) -> String {
    let chars = line.chars().collect::<Vec<_>>();
    if chars.len() <= MAX_SEARCH_PREVIEW_CHARS {
        return line.to_string();
    }

    let match_start = line[..match_start.min(line.len())].chars().count();
    let match_end = line[..match_end.min(line.len())].chars().count();
    let match_center = (match_start + match_end) / 2;

    let mut start = 0;
    let mut end = chars.len();
    for _ in 0..3 {
        let prefix_len = if start > 0 {
            ELLIPSIS.chars().count()
        } else {
            0
        };
        let suffix_len = if end < chars.len() {
            ELLIPSIS.chars().count()
        } else {
            0
        };
        let budget = MAX_SEARCH_PREVIEW_CHARS - prefix_len - suffix_len;
        start = match_center.saturating_sub(budget / 2);
        if start + budget > chars.len() {
            start = chars.len() - budget;
        }
        end = start + budget;
    }

    let mut preview = String::new();
    if start > 0 {
        preview.push_str(ELLIPSIS);
    }
    preview.extend(chars[start..end].iter());
    if end < chars.len() {
        preview.push_str(ELLIPSIS);
    }
    preview
}
