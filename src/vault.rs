use camino::{Utf8Path, Utf8PathBuf};
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::{WalkBuilder, WalkState};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    sync::{Arc, Mutex},
};

use crate::cli;

pub const DEFAULT_MAX_READ_NOTE_CHARS: usize = 4 * 1024;
#[cfg(feature = "attachments")]
pub const DEFAULT_MAX_READ_ATTACHMENT_CHARS: usize = 16 * 1024;

/// A bounded view over an Obsidian-style Markdown vault with atomic note writes.
/// Vault 是一个受限、可即时扫描并支持原子笔记写入的 Obsidian Markdown 工作空间。
#[derive(Debug)]
pub struct Vault {
    pub root: Utf8PathBuf,
    config: arc_swap::ArcSwap<VaultConfig>,
}

impl Vault {
    pub fn open(root: &Utf8PathBuf, config: VaultConfig) -> Result<Self, VaultError> {
        if !root.is_dir() {
            return Err(VaultError::RootIsNotDirectory(root.to_string()));
        }
        Ok(Self {
            root: root.clone(),
            config: arc_swap::ArcSwap::from(Arc::new(config)),
        })
    }

    pub fn config(&self) -> arc_swap::Guard<Arc<VaultConfig>> {
        self.config.load()
    }

    #[cfg(test)]
    pub fn modify_config(&self, modifier: impl FnOnce(&mut VaultConfig)) {
        let guard = self.config.load();
        let mut new_config = (*guard).as_ref().clone();
        drop(guard);
        modifier(&mut new_config);
        self.config.store(Arc::new(new_config));
    }

    pub fn resolve_path(&self, input: &str) -> Result<Utf8PathBuf, VaultError> {
        let path = Utf8Path::new(input);
        if path.is_absolute() {
            return Err(VaultError::AbsolutePathNotAllowed);
        }
        let candidate = self.root.join(path);
        let mut normalized = normalize_path(&candidate);
        if !normalized.starts_with(&self.root) {
            return Err(VaultError::PathEscapesVault);
        }
        if normalized.extension().is_none() {
            normalized.set_extension("md");
        }
        Ok(normalized)
    }

    pub fn resolve_exact_note_path(&self, input: &str) -> Result<Utf8PathBuf, VaultError> {
        if !input.ends_with(".md") {
            return Err(VaultError::ExactNotePathRequiresMarkdownExtension(
                input.to_string(),
            ));
        }
        self.resolve_path(input)
    }

    #[cfg(feature = "attachments")]
    /// Resolves an existing project-relative attachment without changing its extension.
    pub fn resolve_attachment_path(&self, input: &str) -> Result<Utf8PathBuf, VaultError> {
        let path = Utf8Path::new(input);
        if path.is_absolute() {
            return Err(VaultError::AbsolutePathNotAllowed);
        }
        if path.extension().is_none() {
            return Err(VaultError::AttachmentPathRequiresExtension(
                input.to_string(),
            ));
        }
        let normalized = normalize_path(&self.root.join(path));
        if !normalized.starts_with(&self.root) {
            return Err(VaultError::PathEscapesVault);
        }
        Ok(normalized)
    }

    pub fn relative_path(&self, path: &Utf8Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string()
            .replace('\\', "/")
    }

    pub fn write_note_atomic(&self, path: &Utf8Path, content: &str) -> Result<(), VaultError> {
        if !path.starts_with(&self.root) {
            return Err(VaultError::PathEscapesVault);
        }
        let parent = path
            .parent()
            .ok_or_else(|| VaultError::Io("note has no parent directory".to_string()))?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)
            .map_err(|err| VaultError::Io(err.to_string()))?;
        temporary
            .write_all(content.as_bytes())
            .map_err(|err| VaultError::Io(err.to_string()))?;
        temporary
            .persist(path)
            .map_err(|err| VaultError::Io(err.error.to_string()))?;
        Ok(())
    }

    pub fn list_notes(&self) -> Result<Vec<NoteFile>, VaultError> {
        let config = self.config.load();
        let include = Arc::new(compile_globs(&config.include)?);
        let exclude = Arc::new(compile_globs(&config.exclude)?);
        let obsidian_ignore = Arc::new(self.obsidian_ignore_filters()?);
        let files = Arc::new(Mutex::new(Vec::new()));
        let error = Arc::new(Mutex::new(None));
        let root = self.root.clone();
        let include_is_empty = config.include.is_empty();
        let mut walker = WalkBuilder::new(&self.root);
        walker
            .hidden(false)
            .git_ignore(true)
            .git_exclude(true)
            .require_git(false)
            .parents(true)
            .follow_links(config.follow_symlinks);

        // Keep filesystem traversal and metadata reads parallel; sorting below preserves
        // the deterministic order required by paged query results.
        walker.build_parallel().run(|| {
            let files = Arc::clone(&files);
            let error = Arc::clone(&error);
            let include = Arc::clone(&include);
            let exclude = Arc::clone(&exclude);
            let obsidian_ignore = Arc::clone(&obsidian_ignore);
            let root = root.clone();
            Box::new(move |entry| {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(err) => {
                        *error.lock().expect("walker error lock") =
                            Some(VaultError::Io(err.to_string()));
                        return WalkState::Quit;
                    }
                };
                if entry.depth() == 0 {
                    return WalkState::Continue;
                }
                let path = match Utf8PathBuf::from_path_buf(entry.path().to_path_buf()) {
                    Ok(path) => path,
                    Err(path) => {
                        *error.lock().expect("walker error lock") =
                            Some(VaultError::NonUtf8Path(path.display().to_string()));
                        return WalkState::Quit;
                    }
                };
                let rel = path
                    .strip_prefix(&root)
                    .unwrap_or(&path)
                    .to_string()
                    .replace('\\', "/");
                let is_dir = entry.file_type().is_some_and(|kind| kind.is_dir());
                let directory_prefix = format!("{rel}/");
                if default_ignored(&rel)
                    || (is_dir && default_ignored(&directory_prefix))
                    || (is_dir && exclude.is_match(&directory_prefix))
                    || exclude.is_match(&rel)
                    || obsidian_ignore.iter().any(|filter| filter.is_match(&rel))
                {
                    return if is_dir {
                        WalkState::Skip
                    } else {
                        WalkState::Continue
                    };
                }
                if !entry.file_type().is_some_and(|kind| kind.is_file())
                    || path.extension() != Some("md")
                {
                    return WalkState::Continue;
                }
                if !include_is_empty && !include.is_match(&rel) {
                    return WalkState::Continue;
                }
                let metadata = match fs::metadata(&path) {
                    Ok(metadata) => metadata,
                    Err(err) => {
                        *error.lock().expect("walker error lock") =
                            Some(VaultError::Io(err.to_string()));
                        return WalkState::Quit;
                    }
                };
                files.lock().expect("note files lock").push(NoteFile {
                    path,
                    relative_path: rel,
                    size_bytes: metadata.len(),
                    modified_unix_ms: metadata
                        .modified()
                        .ok()
                        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|duration| duration.as_millis() as u64),
                });
                WalkState::Continue
            })
        });
        if let Some(error) = error.lock().expect("walker error lock").take() {
            return Err(error);
        }
        let mut files = Arc::try_unwrap(files)
            .expect("parallel walker still owns note files")
            .into_inner()
            .expect("note files lock");
        files.sort_by(|a, b| natord::compare(&a.relative_path, &b.relative_path));
        Ok(files)
    }

    fn obsidian_ignore_filters(&self) -> Result<Vec<ObsidianIgnoreFilter>, VaultError> {
        let path = self.root.join(".obsidian/app.json");
        if !path.is_file() {
            return Ok(Vec::new());
        }
        let content = fs::read_to_string(&path).map_err(|err| VaultError::Io(err.to_string()))?;
        let config: ObsidianAppConfig = serde_json::from_str(&content)
            .map_err(|err| VaultError::InvalidObsidianAppConfig(err.to_string()))?;
        config
            .user_ignore_filters
            .into_iter()
            .filter_map(|filter| ObsidianIgnoreFilter::parse(&filter))
            .collect()
    }
}

#[derive(Debug, Deserialize)]
struct ObsidianAppConfig {
    #[serde(default, rename = "userIgnoreFilters")]
    user_ignore_filters: Vec<String>,
}

#[derive(Clone)]
enum ObsidianIgnoreFilter {
    Path(String),
    Regex(Regex),
}

impl ObsidianIgnoreFilter {
    fn parse(filter: &str) -> Option<Result<Self, VaultError>> {
        if filter.len() > 1 && filter.starts_with('/') && filter.ends_with('/') {
            let pattern = &filter[1..filter.len() - 1];
            return Some(
                Regex::new(pattern)
                    .map(Self::Regex)
                    .map_err(|err| VaultError::InvalidObsidianIgnoreFilter(err.to_string())),
            );
        }
        let path = filter.trim_matches('/');
        (!path.is_empty()).then(|| Ok(Self::Path(path.to_string())))
    }

    fn is_match(&self, relative_path: &str) -> bool {
        match self {
            Self::Path(path) => {
                relative_path == path || relative_path.starts_with(&format!("{path}/"))
            }
            Self::Regex(regex) => regex.is_match(relative_path),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VaultConfig {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub follow_symlinks: bool,

    pub max_note_bytes: usize,
    pub max_read_note_chars: usize,
    pub max_results: usize,
    pub parse_cache_ttl_secs: u64,
    pub parse_cache_max_entries: usize,

    pub chapters: Option<ChapterConfig>,
}

impl VaultConfig {
    pub(crate) fn build(cli: &cli::Cli) -> Self {
        VaultConfig {
            include: cli.include.clone(),
            exclude: cli.exclude.clone(),
            follow_symlinks: cli.follow_symlinks,
            max_read_note_chars: cli.max_read_note_chars,
            max_results: cli.max_results,
            parse_cache_ttl_secs: cli.parse_cache_ttl_secs,
            parse_cache_max_entries: cli.parse_cache_max_entries,
            ..Default::default()
        }
    }
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self {
            include: Vec::new(),
            exclude: Vec::new(),
            follow_symlinks: false,
            max_note_bytes: 8 * 1024 * 1024,
            max_read_note_chars: DEFAULT_MAX_READ_NOTE_CHARS,
            max_results: 50,
            parse_cache_ttl_secs: 600,
            parse_cache_max_entries: 1024,
            chapters: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChapterConfig {
    pub glob: String,
    pub order: ChapterOrder,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ChapterOrder {
    NaturalPath,
    LexicalPath,
}

#[derive(Clone, Debug, Serialize)]
pub struct NoteFile {
    pub path: Utf8PathBuf,
    pub relative_path: String,
    pub size_bytes: u64,
    pub modified_unix_ms: Option<u64>,
}

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("vault root is not a directory: {0}")]
    RootIsNotDirectory(String),
    #[error("absolute paths are not allowed")]
    AbsolutePathNotAllowed,
    #[error("path escapes vault root")]
    PathEscapesVault,
    #[error("exact note path must end with .md: {0}")]
    ExactNotePathRequiresMarkdownExtension(String),
    #[cfg(feature = "attachments")]
    #[error("attachment path must include its extension: {0}")]
    AttachmentPathRequiresExtension(String),
    #[error("non-utf8 path: {0}")]
    NonUtf8Path(String),
    #[error("invalid glob: {0}")]
    InvalidGlob(String),
    #[error("invalid .obsidian/app.json: {0}")]
    InvalidObsidianAppConfig(String),
    #[error("invalid Obsidian user ignore filter: {0}")]
    InvalidObsidianIgnoreFilter(String),
    #[error("io error: {0}")]
    Io(String),
}

fn normalize_path(path: &Utf8Path) -> Utf8PathBuf {
    let mut out = Utf8PathBuf::new();
    for component in path.components() {
        match component {
            camino::Utf8Component::CurDir => {}
            camino::Utf8Component::ParentDir => {
                out.pop();
            }
            _ => out.push(component.as_str()),
        }
    }
    out
}

fn compile_globs(patterns: &[String]) -> Result<GlobSet, VaultError> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern).map_err(|err| VaultError::InvalidGlob(err.to_string()))?);
    }
    builder
        .build()
        .map_err(|err| VaultError::InvalidGlob(err.to_string()))
}

fn default_ignored(rel: &str) -> bool {
    has_hidden_segment(rel)
        || rel.starts_with("target/")
        || rel.contains("/target/")
        || rel.contains("/.cache/")
        || rel.contains("/node_modules/")
}

fn has_hidden_segment(rel: &str) -> bool {
    rel.split('/').any(|segment| segment.starts_with('.'))
}

#[cfg(test)]
mod tests;
