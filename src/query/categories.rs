use std::collections::{BTreeMap, BTreeSet};

use super::path_filter::PathFilter;
use super::{CategoryOutputBucket, GetCategoriesResult, ListCategoriesResult, VaultQueries};

impl VaultQueries {
    pub fn list_categories(
        &self,
        include: &[String],
        exclude: &[String],
    ) -> anyhow::Result<ListCategoriesResult> {
        let buckets = self.collect_categories(None, include, exclude)?;
        Ok(ListCategoriesResult {
            categories: buckets.into_keys().collect(),
        })
    }

    pub fn get_categories(
        &self,
        categories: &[String],
        include: &[String],
        exclude: &[String],
    ) -> anyhow::Result<GetCategoriesResult> {
        let buckets = self.collect_categories(Some(categories), include, exclude)?;
        Ok(GetCategoriesResult {
            categories: buckets
                .into_iter()
                .map(|(category, files)| CategoryOutputBucket {
                    category,
                    files: files.into_iter().collect(),
                })
                .collect(),
        })
    }

    fn collect_categories(
        &self,
        categories: Option<&[String]>,
        include: &[String],
        exclude: &[String],
    ) -> anyhow::Result<BTreeMap<String, BTreeSet<String>>> {
        let filter = PathFilter::new(include, exclude)?;
        let mut buckets: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for note in self.index_filtered_notes(&filter)? {
            for category in note_categories(&note.file.relative_path) {
                if category_matches(categories, &category) {
                    buckets
                        .entry(category)
                        .or_default()
                        .insert(note.file.relative_path.clone());
                }
            }
        }
        Ok(buckets)
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

fn category_matches(wanted: Option<&[String]>, found: &str) -> bool {
    wanted.is_none_or(|wanted| {
        wanted
            .iter()
            .any(|category| normalize_category(category) == found)
    })
}

fn normalize_category(category: &str) -> String {
    category.trim().trim_matches('/').to_string()
}
