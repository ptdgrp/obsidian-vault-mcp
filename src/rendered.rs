//! Read-only rendered content has its own pagination, never source line locators.
use crate::{
    query::{McpNonNegativeInteger, VaultQueries},
    vault::Vault,
};
use anyhow::{Context, bail};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub fn default_page() -> usize {
    1
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct RenderPagination {
    #[schemars(with = "McpNonNegativeInteger")]
    pub page: usize,
    #[schemars(with = "McpNonNegativeInteger")]
    pub total_pages: usize,
    /// Canvas nodes/connections/memberships, or Base rows/empty-group markers, across all pages.
    #[schemars(with = "McpNonNegativeInteger")]
    pub total_units: usize,
    #[schemars(with = "McpNonNegativeInteger")]
    pub returned_units: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<McpNonNegativeInteger>")]
    pub next_page: Option<usize>,
}

/// Pack complete semantic units. A single unit plus its necessary context may
/// exceed the budget; the same budget must be used when following next_page.
pub fn paginate<T>(
    items: &[T],
    page: usize,
    budget: usize,
    render: impl Fn(&[T]) -> String,
) -> anyhow::Result<(String, RenderPagination, std::ops::Range<usize>)> {
    if page == 0 {
        bail!("page must be greater than or equal to 1");
    }
    let mut spans = Vec::new();
    let mut start = 0;
    while start < items.len() {
        let mut end = start + 1;
        // Render exponentially growing candidates, then binary-search the
        // final boundary. Rendering every successive prefix is quadratic for
        // a large requested page; these labels/rows grow monotonically.
        if render(&items[start..end]).chars().count() <= budget {
            let mut failed = items.len() + 1;
            let mut width = 2usize;
            loop {
                let candidate = start.saturating_add(width).min(items.len());
                if candidate <= end {
                    break;
                }
                if render(&items[start..candidate]).chars().count() > budget {
                    failed = candidate;
                    break;
                }
                end = candidate;
                if end == items.len() {
                    break;
                }
                width = width.saturating_mul(2);
            }
            while end + 1 < failed {
                let candidate = end + (failed - end) / 2;
                if render(&items[start..candidate]).chars().count() <= budget {
                    end = candidate;
                } else {
                    failed = candidate;
                }
            }
        }
        spans.push(start..end);
        start = end;
    }
    if spans.is_empty() {
        spans.push(0..0);
    }
    let total_pages = spans.len();
    let span = spans
        .get(page - 1)
        .cloned()
        .unwrap_or(items.len()..items.len());
    let content = render(&items[span.clone()]);
    Ok((
        content,
        RenderPagination {
            page,
            total_pages,
            total_units: items.len(),
            returned_units: span.len(),
            next_page: (page < total_pages).then_some(page + 1),
        },
        span,
    ))
}

pub fn read_source(
    vault: &Vault,
    requested: &str,
    extension: &str,
) -> anyhow::Result<(String, String)> {
    if camino::Utf8Path::new(requested).extension() != Some(extension) {
        bail!("exact file path must end with .{extension}: {requested}");
    }
    let path = vault.resolve_existing_file(requested)?;
    let maximum = vault.config().max_note_bytes;
    if std::fs::metadata(&path)?.len() > maximum as u64 {
        bail!("file exceeds configured maximum size ({maximum} bytes): {requested}");
    }
    let content = std::fs::read_to_string(&path).with_context(|| format!("reading {requested}"))?;
    if content.len() > maximum {
        bail!("file exceeds configured maximum size: {requested}");
    }
    Ok((vault.relative_path(&path), content))
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct ListFilesRequest {
    /// Extensions without a dot, e.g. ["base", "canvas"]. Empty means all file types.
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default = "default_page")]
    #[schemars(with = "McpNonNegativeInteger", range(min = 1))]
    pub page: usize,
    /// Files per page, 1-100; defaults to 100.
    #[schemars(with = "Option<McpNonNegativeInteger>", range(min = 1, max = 100))]
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct FileSummary {
    pub path: String,
    pub extension: String,
    #[schemars(with = "McpNonNegativeInteger")]
    pub size_bytes: u64,
}
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct ListFilesResult {
    pub files: Vec<FileSummary>,
    pub pagination: FilesPagination,
}
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct FilesPagination {
    #[schemars(with = "McpNonNegativeInteger")]
    pub page: usize,
    #[schemars(with = "McpNonNegativeInteger")]
    pub total_pages: usize,
    #[schemars(with = "McpNonNegativeInteger")]
    pub total_files: usize,
}
impl VaultQueries {
    pub fn list_files(&self, request: ListFilesRequest) -> anyhow::Result<ListFilesResult> {
        let limit = request.limit.unwrap_or(100);
        if !(1..=100).contains(&limit) {
            bail!("limit must be between 1 and 100");
        }
        if request
            .extensions
            .iter()
            .any(|ext| ext.is_empty() || ext.contains(['.', '/', '\\']))
        {
            bail!("extensions must be non-empty file extensions without dots or slashes");
        }
        let filter =
            crate::query::path_filter::PathFilter::new(&request.include, &request.exclude)?;
        let files: Vec<_> = self
            .vault
            .list_files()?
            .into_iter()
            .filter(|file| {
                filter.is_match(&file.relative_path)
                    && (request.extensions.is_empty()
                        || request.extensions.iter().any(|ext| {
                            file.path
                                .extension()
                                .unwrap_or("")
                                .eq_ignore_ascii_case(ext)
                        }))
            })
            .collect();
        let page = crate::query::public::PageSlice::new(files, request.page, limit)?;
        let total_files = page.total_items();
        let pagination = FilesPagination {
            page: request.page,
            total_pages: page.pagination().total_pages,
            total_files,
        };
        Ok(ListFilesResult {
            files: page
                .into_items()
                .into_iter()
                .map(|file| FileSummary {
                    extension: file.path.extension().unwrap_or("").into(),
                    path: file.relative_path,
                    size_bytes: file.size_bytes,
                })
                .collect(),
            pagination,
        })
    }
}
