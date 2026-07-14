use std::collections::BTreeSet;

use super::path_filter::PathFilter;
use super::public::PageSlice;
use super::{
    GetCategoryPagination, GetCategoryResult, ListCategoriesPagination, ListCategoriesResult,
    VaultQueries,
};

const CATEGORY_PAGE_SIZE: usize = 100;

impl VaultQueries {
    pub fn list_categories(
        &self,
        include: &[String],
        exclude: &[String],
        page: usize,
    ) -> anyhow::Result<ListCategoriesResult> {
        let categories = self.collect_category_names(include, exclude)?;
        let slice = PageSlice::new(categories, page, CATEGORY_PAGE_SIZE)?;
        let total_categories = slice.total_items();
        let pagination = slice.pagination();
        Ok(ListCategoriesResult {
            categories: slice.into_items(),
            pagination: ListCategoriesPagination {
                page: pagination.page,
                total_pages: pagination.total_pages,
                total_categories,
            },
        })
    }

    pub fn get_category(
        &self,
        category: &str,
        include: &[String],
        exclude: &[String],
        page: usize,
    ) -> anyhow::Result<GetCategoryResult> {
        let category = normalize_category_query(category)?;
        let filter = PathFilter::new(include, exclude)?;
        let mut notes = self
            .index_filtered_notes(&filter)?
            .into_iter()
            .filter(|note| note_categories(&note.file.relative_path).contains(&category))
            .map(|note| note.file.relative_path)
            .collect::<Vec<_>>();
        notes.sort_by(|a, b| natord::compare(a, b));

        let slice = PageSlice::new(notes, page, CATEGORY_PAGE_SIZE)?;
        let total_notes = slice.total_items();
        let pagination = slice.pagination();
        Ok(GetCategoryResult {
            notes: slice.into_items(),
            pagination: GetCategoryPagination {
                page: pagination.page,
                total_pages: pagination.total_pages,
                total_notes,
            },
        })
    }

    fn collect_category_names(
        &self,
        include: &[String],
        exclude: &[String],
    ) -> anyhow::Result<Vec<String>> {
        let filter = PathFilter::new(include, exclude)?;
        let mut categories = BTreeSet::new();
        for note in self.index_filtered_notes(&filter)? {
            for category in note_categories(&note.file.relative_path) {
                categories.insert(category);
            }
        }
        let mut categories = categories.into_iter().collect::<Vec<_>>();
        categories.sort_by(|a, b| natord::compare(a, b));
        Ok(categories)
    }
}

fn note_categories(relative_path: &str) -> BTreeSet<String> {
    relative_path
        .rsplit_once('/')
        .map(|(parent, _)| {
            parent
                .split('/')
                .map(normalize_category)
                .filter(|category| !category.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn normalize_category(category: &str) -> String {
    category.trim().trim_matches('/').to_string()
}

fn normalize_category_query(category: &str) -> anyhow::Result<String> {
    let category = normalize_category(category);
    if category.is_empty() {
        anyhow::bail!("provide a non-empty category");
    }
    if category.contains('/') {
        anyhow::bail!("category must be a single folder name, not a path");
    }
    Ok(category)
}
