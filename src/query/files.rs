use std::collections::BTreeSet;
use std::fs;

use camino::Utf8PathBuf;
use chrono::{DateTime, Local};
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;

use crate::parser::ParsedNote;

use super::{
    VaultFile, VaultFileKind, VaultFilesOptions, VaultFilesResult, VaultFilesSummary, VaultQueries,
};

impl VaultQueries {
    pub fn list_vault_files(&self, options: VaultFilesOptions) -> anyhow::Result<VaultFilesResult> {
        let include = compile_file_globs(&self.vault.config.include)?;
        let exclude = compile_file_globs(&self.vault.config.exclude)?;
        let mut files = Vec::new();
        let mut directories = BTreeSet::new();
        let mut note_count = 0;
        let mut attachment_count = 0;
        let mut walker = WalkBuilder::new(&self.vault.root);
        walker
            .hidden(false)
            .git_ignore(true)
            .git_exclude(true)
            .require_git(false)
            .parents(true)
            .follow_links(self.vault.config.follow_symlinks);

        for entry in walker.build() {
            let entry = entry?;
            let path = Utf8PathBuf::from_path_buf(entry.path().to_path_buf())
                .map_err(|path| anyhow::anyhow!("non-utf8 path: {}", path.display()))?;
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }

            let relative_path = self.vault.relative_path(&path);
            if should_ignore_file_path(&relative_path) || exclude.is_match(&relative_path) {
                continue;
            }

            let is_note = path.extension() == Some("md");
            if is_note && !self.vault.config.include.is_empty() && !include.is_match(&relative_path)
            {
                continue;
            }

            if is_note {
                note_count += 1;
            } else {
                attachment_count += 1;
            }

            if let Some(parent) = parent_directory(&relative_path) {
                directories.insert(parent);
            }

            if is_note && !options.include_files {
                continue;
            }
            if !is_note && !options.include_attachments {
                continue;
            }

            files.push(self.file_entry(path, relative_path, is_note, &options)?);
        }

        files.sort_by(|a, b| natord::compare(&a.path, &b.path));

        let summary = VaultFilesSummary {
            notes: note_count,
            directories: directories.len(),
            attachments: attachment_count,
            empty_directories: 0,
        };
        let total_files = files.len();
        let truncated_files = total_files.saturating_sub(options.max_files);
        files.truncate(options.max_files);

        Ok(VaultFilesResult {
            summary,
            files,
            truncated_files,
        })
    }

    fn file_entry(
        &self,
        path: Utf8PathBuf,
        relative_path: String,
        is_note: bool,
        options: &VaultFilesOptions,
    ) -> anyhow::Result<VaultFile> {
        let metadata = fs::metadata(&path)?;
        let parsed = is_note
            .then(|| self.parse_file_cached(&path, relative_path.clone()).ok())
            .flatten();
        Ok(VaultFile {
            path: relative_path,
            kind: if is_note {
                VaultFileKind::Note
            } else {
                VaultFileKind::Attachment
            },
            title: parsed.as_deref().and_then(note_title),
            outline: options
                .include_readme_outline
                .then(|| parsed.as_deref().map(note_outline).unwrap_or_default())
                .filter(|_| path.file_name() == Some("README.md")),
            size: human_size(metadata.len()),
            modified: metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .and_then(|duration| local_datetime(duration.as_millis() as u64)),
        })
    }
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }

    let mut size = bytes as f64;
    let mut unit_index = 0;
    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    if size >= 10.0 {
        format!("{size:.0} {}", UNITS[unit_index])
    } else {
        format!("{size:.1} {}", UNITS[unit_index])
    }
}

fn local_datetime(unix_ms: u64) -> Option<String> {
    let seconds = i64::try_from(unix_ms / 1000).ok()?;
    let nanos = u32::try_from((unix_ms % 1000) * 1_000_000).ok()?;
    let datetime: DateTime<Local> = DateTime::from_timestamp(seconds, nanos)?.into();
    Some(datetime.format("%Y-%m-%d %H:%M:%S").to_string())
}

fn compile_file_globs(patterns: &[String]) -> anyhow::Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern)?);
    }
    Ok(builder.build()?)
}

fn note_title(note: &ParsedNote) -> Option<String> {
    note.headings.first().map(|heading| heading.text.clone())
}

fn note_outline(note: &ParsedNote) -> Vec<String> {
    note.headings
        .iter()
        .map(|heading| heading.text.clone())
        .collect()
}

fn parent_directory(relative_path: &str) -> Option<String> {
    relative_path
        .rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
        .or_else(|| Some(String::new()))
}

fn should_ignore_file_path(relative_path: &str) -> bool {
    relative_path
        .split('/')
        .any(|segment| segment.starts_with('.'))
        || relative_path.starts_with("target/")
        || relative_path.contains("/target/")
        || relative_path.contains("/.cache/")
        || relative_path.contains("/node_modules/")
}
