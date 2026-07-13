use anyhow::Context;
use globset::{Glob, GlobSet, GlobSetBuilder};

#[derive(Debug)]
pub(super) struct PathFilter {
    include: GlobSet,
    exclude: GlobSet,
    include_is_empty: bool,
}

impl PathFilter {
    pub(super) fn new(include: &[String], exclude: &[String]) -> anyhow::Result<Self> {
        Ok(Self {
            include: compile_glob_set(include, "include")?,
            exclude: compile_glob_set(exclude, "exclude")?,
            include_is_empty: include.is_empty(),
        })
    }

    pub(super) fn is_match(&self, relative_path: &str) -> bool {
        (self.include_is_empty || self.include.is_match(relative_path))
            && !self.exclude.is_match(relative_path)
    }
}

fn compile_glob_set(patterns: &[String], field: &str) -> anyhow::Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder
            .add(Glob::new(pattern).with_context(|| format!("invalid {field} glob {pattern:?}"))?);
    }
    builder
        .build()
        .with_context(|| format!("invalid {field} glob set"))
}
