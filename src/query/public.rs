use anyhow::bail;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Pagination {
    pub page: usize,
    pub total_pages: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct PageSlice<T> {
    items: Vec<T>,
    total_items: usize,
    pagination: Pagination,
}

impl<T> PageSlice<T> {
    pub(crate) fn new(items: Vec<T>, page: usize, size: usize) -> anyhow::Result<Self> {
        if page == 0 {
            bail!("page must be greater than or equal to 1");
        }
        assert!(size > 0, "page size must be positive");

        let total_items = items.len();
        let total_pages = total_items.div_ceil(size);
        let start = page.saturating_sub(1).saturating_mul(size);
        let items = items.into_iter().skip(start).take(size).collect();

        Ok(Self {
            items,
            total_items,
            pagination: Pagination { page, total_pages },
        })
    }

    pub(crate) fn items(&self) -> &[T] {
        &self.items
    }

    pub(crate) fn total_items(&self) -> usize {
        self.total_items
    }

    pub(crate) fn pagination(&self) -> Pagination {
        self.pagination
    }
}

pub(crate) struct Locator;

impl Locator {
    pub(crate) fn lines(path: &str, start: u64, end: u64) -> String {
        if start == end {
            format!("{path}#L{start}")
        } else {
            format!("{path}#L{start}-L{end}")
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedReference {
    pub path: String,
    pub heading_path: Vec<String>,
    pub block_id: Option<String>,
}

impl ResolvedReference {
    pub(crate) fn heading(path: impl Into<String>, heading_path: Vec<String>) -> Self {
        Self {
            path: path.into(),
            heading_path,
            block_id: None,
        }
    }

    pub(crate) fn block(path: impl Into<String>, block_id: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            heading_path: Vec::new(),
            block_id: Some(block_id.into()),
        }
    }

    pub(crate) fn format(&self) -> String {
        if let Some(block_id) = &self.block_id {
            return format!("{}#^{block_id}", self.path);
        }
        let mut result = self.path.clone();
        for heading in &self.heading_path {
            result.push('#');
            result.push_str(heading);
        }
        result
    }

    pub(crate) fn contains(&self, other: &Self) -> bool {
        self.path == other.path
            && match (&self.block_id, &other.block_id) {
                (Some(left), Some(right)) => left == right,
                (None, None) => other.heading_path.starts_with(&self.heading_path),
                _ => false,
            }
    }
}
