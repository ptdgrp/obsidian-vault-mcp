use std::{fs, io::Write};

use camino::{Utf8Path, Utf8PathBuf};
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};

pub const DEFAULT_MAX_READ_NOTE_BYTES: usize = 4 * 1024;

/// A bounded view over an Obsidian-style Markdown vault with atomic note writes.
/// Vault 是一个受限、可即时扫描并支持原子笔记写入的 Obsidian Markdown 工作空间。
#[derive(Clone, Debug)]
pub struct Vault {
    pub root: Utf8PathBuf,
    pub config: VaultConfig,
}

impl Vault {
    pub fn open(root: Utf8PathBuf, config: VaultConfig) -> Result<Self, VaultError> {
        if !root.is_dir() {
            return Err(VaultError::RootIsNotDirectory(root.to_string()));
        }
        Ok(Self { root, config })
    }

    pub fn resolve_path(&self, input: &str) -> Result<Utf8PathBuf, VaultError> {
        let path = Utf8Path::new(input);
        if path.is_absolute() {
            return Err(VaultError::AbsolutePathNotAllowed);
        }
        let candidate = self.root.join(path);
        let normalized = normalize_path(&candidate);
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

    pub fn read_note(&self, note: &str) -> Result<(Utf8PathBuf, String), VaultError> {
        let mut path = self.resolve_path(note)?;
        if path.extension().is_none() {
            path.set_extension("md");
        }
        if !path.is_file() {
            return Err(VaultError::NoteNotFound(note.to_string()));
        }
        let metadata = fs::metadata(&path).map_err(|err| VaultError::Io(err.to_string()))?;
        if metadata.len() as usize > self.config.max_note_bytes {
            return Err(VaultError::NoteTooLarge {
                path: self.relative_path(&path),
                limit: self.config.max_note_bytes,
                actual: metadata.len() as usize,
            });
        }
        let content = fs::read_to_string(&path).map_err(|err| VaultError::Io(err.to_string()))?;
        Ok((path, content))
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
        let include = compile_globs(&self.config.include)?;
        let exclude = compile_globs(&self.config.exclude)?;
        let mut files = Vec::new();
        let mut walker = WalkBuilder::new(&self.root);
        walker
            .hidden(false)
            .git_ignore(true)
            .git_exclude(true)
            .require_git(false)
            .parents(true)
            .follow_links(self.config.follow_symlinks);

        for entry in walker.build() {
            let entry = entry.map_err(|err| VaultError::Io(err.to_string()))?;
            let path = Utf8PathBuf::from_path_buf(entry.path().to_path_buf())
                .map_err(|path| VaultError::NonUtf8Path(path.display().to_string()))?;
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }
            if path.extension() != Some("md") {
                continue;
            }
            let rel = self.relative_path(&path);
            if default_ignored(&rel) || exclude.is_match(&rel) {
                continue;
            }
            if !self.config.include.is_empty() && !include.is_match(&rel) {
                continue;
            }
            let metadata = fs::metadata(&path).map_err(|err| VaultError::Io(err.to_string()))?;
            files.push(NoteFile {
                path,
                relative_path: rel,
                size_bytes: metadata.len(),
                modified_unix_ms: metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|duration| duration.as_millis() as u64),
            });
        }
        files.sort_by(|a, b| natord::compare(&a.relative_path, &b.relative_path));
        Ok(files)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VaultConfig {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub follow_symlinks: bool,

    pub max_note_bytes: usize,
    pub max_output_bytes: usize,
    pub max_read_note_bytes: usize,
    pub max_results: usize,
    pub parse_cache_ttl_secs: u64,
    pub parse_cache_max_entries: usize,

    pub chapters: Option<ChapterConfig>,
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self {
            include: Vec::new(),
            exclude: Vec::new(),
            follow_symlinks: false,
            max_note_bytes: 8 * 1024 * 1024,
            max_output_bytes: 262_144,
            max_read_note_bytes: DEFAULT_MAX_READ_NOTE_BYTES,
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
    #[error("note not found: {0}")]
    NoteNotFound(String),
    #[error("note is too large: {path} ({actual} > {limit} bytes)")]
    NoteTooLarge {
        path: String,
        limit: usize,
        actual: usize,
    },
    #[error("non-utf8 path: {0}")]
    NonUtf8Path(String),
    #[error("invalid glob: {0}")]
    InvalidGlob(String),
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
