use std::collections::BTreeMap;

use camino::Utf8PathBuf;

use super::{ApplyPatchResult, EditNoteResult, VaultMutations};
use crate::{
    parser::{NoteParser, ReferenceInfo},
    resolver::{IndexedNote, RefResolver, ResolveResult},
};

struct PlannedEdit {
    path: Utf8PathBuf,
    original: String,
    updated: String,
}

impl VaultMutations {
    pub fn edit_note(
        &self,
        path: &str,
        old_text: &str,
        new_text: &str,
        dry_run: bool,
    ) -> anyhow::Result<EditNoteResult> {
        if old_text.is_empty() {
            anyhow::bail!("old_text must not be empty");
        }
        let (path, relative_path, original) = self.read_editable_note(path)?;
        let mut matches = Vec::new();
        let mut scan = 0;
        while let Some(offset) = original[scan..].find(old_text) {
            let start = scan + offset;
            matches.push(start);
            scan = start
                + original[start..]
                    .chars()
                    .next()
                    .expect("match has a character")
                    .len_utf8();
        }
        let [start] = matches.as_slice() else {
            anyhow::bail!(
                "old_text must match exactly once in `{relative_path}`; found {} matches",
                matches.len()
            );
        };
        let mut updated = original.clone();
        updated.replace_range(*start..*start + old_text.len(), new_text);
        let mut edits = BTreeMap::new();
        edits.insert(
            relative_path.clone(),
            PlannedEdit {
                path,
                original,
                updated,
            },
        );
        self.apply_planned_edits(edits, dry_run)?;
        Ok(EditNoteResult {
            path: relative_path,
            dry_run,
        })
    }

    pub fn apply_patch(&self, patch: &str, dry_run: bool) -> anyhow::Result<ApplyPatchResult> {
        let file_patches = parse_unified_patch(patch)?;
        let mut edits = BTreeMap::new();
        for file_patch in file_patches {
            let (path, relative_path, original) = self.read_editable_note(&file_patch.path)?;
            if edits.contains_key(&relative_path) {
                anyhow::bail!("patch contains duplicate note: {relative_path}");
            }
            let updated = apply_file_patch(&original, &file_patch)?;
            edits.insert(
                relative_path,
                PlannedEdit {
                    path,
                    original,
                    updated,
                },
            );
        }
        self.apply_planned_edits(edits, dry_run)
    }

    fn read_editable_note(&self, path: &str) -> anyhow::Result<(Utf8PathBuf, String, String)> {
        let file = self.queries.vault.resolve_exact_note_path(path)?;
        let relative_path = self.queries.vault.relative_path(&file);
        let metadata = std::fs::symlink_metadata(&file)
            .map_err(|error| anyhow::anyhow!("note not found: {path}: {error}"))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            anyhow::bail!("not a regular Markdown note: {path}");
        }
        let root = std::fs::canonicalize(&self.queries.vault.root)?;
        if !std::fs::canonicalize(&file)?.starts_with(root) {
            anyhow::bail!("note path escapes vault root: {path}");
        }
        let original = self.queries.read_note_content(&file)?;
        Ok((file, relative_path, original))
    }

    fn apply_planned_edits(
        &self,
        edits: BTreeMap<String, PlannedEdit>,
        dry_run: bool,
    ) -> anyhow::Result<ApplyPatchResult> {
        let maximum = self.queries.vault.config().max_note_bytes;
        for (relative_path, edit) in &edits {
            if edit.updated == edit.original {
                anyhow::bail!("note edit produced no changes for `{relative_path}`");
            }
            if edit.updated.len() > maximum {
                anyhow::bail!(
                    "note exceeds configured maximum size: {} > {maximum} bytes",
                    edit.updated.len()
                );
            }
        }
        self.check_preserved_references(&edits)?;
        for (relative_path, edit) in &edits {
            if std::fs::read(&edit.path)? != edit.original.as_bytes() {
                anyhow::bail!("note changed on disk before edit: `{relative_path}`");
            }
        }
        let mut changed_notes = Vec::new();
        for (relative_path, edit) in &edits {
            if !dry_run {
                self.write_note_atomic(&edit.path, relative_path, &edit.updated)?;
            }
            changed_notes.push(relative_path.clone());
        }
        Ok(ApplyPatchResult {
            changed_notes,
            dry_run,
        })
    }

    fn check_preserved_references(
        &self,
        edits: &BTreeMap<String, PlannedEdit>,
    ) -> anyhow::Result<()> {
        let original_notes = self.queries.index_notes()?;
        let mut updated_notes = original_notes.clone();
        for (relative_path, edit) in edits {
            let Some(note) = updated_notes
                .iter_mut()
                .find(|note| note.file.relative_path == *relative_path)
            else {
                anyhow::bail!("note is not visible in vault: {relative_path}");
            };
            note.parsed = std::sync::Arc::new(NoteParser::parse(
                relative_path,
                &edit.updated,
                self.queries.vault.config().max_note_bytes,
            )?);
        }
        let mut broken = Vec::new();
        for old_source in &original_notes {
            let new_source = updated_notes
                .iter()
                .find(|note| note.file.relative_path == old_source.file.relative_path)
                .expect("updated note has original path");
            let mut remaining = all_references(new_source);
            for (target, reference, raw, source) in all_references(old_source) {
                if edits.contains_key(&old_source.file.relative_path) {
                    let Some(position) = remaining.iter().position(|candidate| {
                        candidate.0 == target && candidate.1 == reference && candidate.2 == raw
                    }) else {
                        continue;
                    };
                    remaining.remove(position);
                }
                let Some(old_target) =
                    reference_target(target, &old_source.file.relative_path, &original_notes)
                else {
                    continue;
                };
                let new_target =
                    reference_target(target, &old_source.file.relative_path, &updated_notes);
                let old_note = original_notes
                    .iter()
                    .find(|note| note.file.relative_path == old_target)
                    .expect("resolved note exists");
                let was_valid = RefResolver::reference_exists(old_note, reference);
                let still_valid = new_target.as_deref() == Some(old_target.as_str())
                    && updated_notes
                        .iter()
                        .find(|note| note.file.relative_path == old_target)
                        .is_some_and(|note| RefResolver::reference_exists(note, reference));
                if was_valid && !still_valid {
                    broken.push(format!("- {}: {raw}", source.path_with_line_ref()));
                }
            }
        }
        if !broken.is_empty() {
            anyhow::bail!("edit would break references:\n{}", broken.join("\n"));
        }
        Ok(())
    }
}

type ReferenceView<'a> = (
    &'a str,
    &'a Option<ReferenceInfo>,
    &'a str,
    &'a crate::parser::SourceSpan,
);

fn all_references(note: &IndexedNote) -> Vec<ReferenceView<'_>> {
    note.parsed
        .links
        .iter()
        .map(|link| (&*link.target, &link.reference, &*link.raw, &link.source))
        .chain(
            note.parsed
                .embeds
                .iter()
                .map(|embed| (&*embed.target, &embed.reference, &*embed.raw, &embed.source)),
        )
        .collect()
}

fn reference_target(target: &str, source_path: &str, notes: &[IndexedNote]) -> Option<String> {
    if target.is_empty() {
        return Some(source_path.to_string());
    }
    match RefResolver::resolve(target, notes) {
        ResolveResult::Resolved { path, .. } => Some(path),
        _ => None,
    }
}

#[derive(Clone)]
struct PatchLine {
    text: String,
    newline: bool,
}

struct HunkLine {
    kind: char,
    line: PatchLine,
}

struct Hunk {
    old_start: usize,
    old_count: usize,
    new_count: usize,
    lines: Vec<HunkLine>,
}

struct FilePatch {
    path: String,
    hunks: Vec<Hunk>,
}

fn parse_unified_patch(patch: &str) -> anyhow::Result<Vec<FilePatch>> {
    let lines = patch.lines().collect::<Vec<_>>();
    let mut cursor = 0;
    let mut files = Vec::new();
    while cursor < lines.len() {
        while cursor < lines.len()
            && (lines[cursor].starts_with("diff --git ")
                || lines[cursor].starts_with("index ")
                || lines[cursor].is_empty())
        {
            cursor += 1;
        }
        if cursor == lines.len() {
            break;
        }
        let old_path = lines[cursor].strip_prefix("--- ").ok_or_else(|| {
            anyhow::anyhow!("expected --- file header at patch line {}", cursor + 1)
        })?;
        cursor += 1;
        let new_path = lines
            .get(cursor)
            .and_then(|line| line.strip_prefix("+++ "))
            .ok_or_else(|| {
                anyhow::anyhow!("expected +++ file header at patch line {}", cursor + 1)
            })?;
        cursor += 1;
        let old_path = patch_path(old_path, "a/")?;
        let new_path = patch_path(new_path, "b/")?;
        if old_path != new_path {
            anyhow::bail!("patch cannot create, delete, or rename notes: {old_path} -> {new_path}");
        }
        let mut hunks = Vec::new();
        while cursor < lines.len() && lines[cursor].starts_with("@@ ") {
            let (old_start, old_count, new_count) = parse_hunk_header(lines[cursor])?;
            cursor += 1;
            let mut hunk_lines: Vec<HunkLine> = Vec::new();
            let mut old_seen = 0;
            let mut new_seen = 0;
            while cursor < lines.len() {
                let line = lines[cursor];
                if old_seen == old_count
                    && new_seen == new_count
                    && line != "\\ No newline at end of file"
                {
                    break;
                }
                if line == "\\ No newline at end of file" {
                    let previous = hunk_lines
                        .last_mut()
                        .ok_or_else(|| anyhow::anyhow!("orphan no-newline marker"))?;
                    previous.line.newline = false;
                } else {
                    let kind = line
                        .chars()
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("empty hunk line"))?;
                    if !matches!(kind, ' ' | '+' | '-') {
                        anyhow::bail!("invalid hunk line: {line}");
                    }
                    old_seen += usize::from(kind != '+');
                    new_seen += usize::from(kind != '-');
                    if old_seen > old_count || new_seen > new_count {
                        anyhow::bail!("hunk line count exceeds header for {old_path}");
                    }
                    hunk_lines.push(HunkLine {
                        kind,
                        line: PatchLine {
                            text: line[1..].to_string(),
                            newline: true,
                        },
                    });
                }
                cursor += 1;
            }
            if old_seen != old_count || new_seen != new_count {
                anyhow::bail!("hunk line count does not match header for {old_path}");
            }
            hunks.push(Hunk {
                old_start,
                old_count,
                new_count,
                lines: hunk_lines,
            });
        }
        if hunks.is_empty() {
            anyhow::bail!("patch has no hunks for {old_path}");
        }
        files.push(FilePatch {
            path: old_path,
            hunks,
        });
    }
    if files.is_empty() {
        anyhow::bail!("patch contains no Markdown file changes");
    }
    Ok(files)
}

fn patch_path(header: &str, prefix: &str) -> anyhow::Result<String> {
    let path = header.split('\t').next().unwrap_or(header);
    let decoded = if path.starts_with('"') {
        decode_git_quoted_path(path)?
    } else {
        path.to_string()
    };
    let path = decoded
        .strip_prefix(prefix)
        .ok_or_else(|| anyhow::anyhow!("expected {prefix} path: {path}"))?;
    if !path.ends_with(".md") {
        anyhow::bail!("patch path must end with .md: {path}");
    }
    Ok(path.to_string())
}

fn decode_git_quoted_path(quoted: &str) -> anyhow::Result<String> {
    let inner = quoted
        .strip_prefix('"')
        .and_then(|path| path.strip_suffix('"'))
        .ok_or_else(|| anyhow::anyhow!("invalid quoted patch path: {quoted}"))?;
    let bytes = inner.as_bytes();
    let mut decoded = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != b'\\' {
            decoded.push(bytes[cursor]);
            cursor += 1;
            continue;
        }
        cursor += 1;
        let escaped = *bytes
            .get(cursor)
            .ok_or_else(|| anyhow::anyhow!("trailing escape in patch path"))?;
        match escaped {
            b'\\' | b'"' => decoded.push(escaped),
            b't' => decoded.push(b'\t'),
            b'n' => decoded.push(b'\n'),
            b'r' => decoded.push(b'\r'),
            b'0'..=b'7' => {
                let digits = bytes
                    .get(cursor..cursor + 3)
                    .ok_or_else(|| anyhow::anyhow!("incomplete octal escape in patch path"))?;
                if !digits.iter().all(|digit| (b'0'..=b'7').contains(digit)) {
                    anyhow::bail!("invalid octal escape in patch path");
                }
                let value = u16::from(digits[0] - b'0') * 64
                    + u16::from(digits[1] - b'0') * 8
                    + u16::from(digits[2] - b'0');
                decoded.push(
                    u8::try_from(value)
                        .map_err(|_| anyhow::anyhow!("octal escape exceeds one byte"))?,
                );
                cursor += 2;
            }
            _ => anyhow::bail!("unsupported escape in patch path"),
        }
        cursor += 1;
    }
    Ok(String::from_utf8(decoded)?)
}

fn parse_hunk_header(header: &str) -> anyhow::Result<(usize, usize, usize)> {
    let parts = header.split_whitespace().collect::<Vec<_>>();
    if parts.len() < 4 || parts[0] != "@@" || parts[3] != "@@" {
        anyhow::bail!("invalid hunk header: {header}");
    }
    let (old_start, old_count) = parse_range(parts[1], '-')?;
    let (_, new_count) = parse_range(parts[2], '+')?;
    Ok((old_start, old_count, new_count))
}

fn parse_range(value: &str, prefix: char) -> anyhow::Result<(usize, usize)> {
    let value = value
        .strip_prefix(prefix)
        .ok_or_else(|| anyhow::anyhow!("invalid hunk range: {value}"))?;
    let (start, count) = value.split_once(',').unwrap_or((value, "1"));
    Ok((start.parse()?, count.parse()?))
}

fn split_lines(content: &str) -> Vec<PatchLine> {
    content
        .split_inclusive('\n')
        .map(|line| PatchLine {
            text: line.strip_suffix('\n').unwrap_or(line).to_string(),
            newline: line.ends_with('\n'),
        })
        .collect()
}

fn apply_file_patch(content: &str, patch: &FilePatch) -> anyhow::Result<String> {
    let original = split_lines(content);
    let mut output = Vec::new();
    let mut cursor = 0;
    for hunk in &patch.hunks {
        let start = if hunk.old_count == 0 {
            hunk.old_start
        } else {
            hunk.old_start
                .checked_sub(1)
                .ok_or_else(|| anyhow::anyhow!("invalid zero line in patch"))?
        };
        if start < cursor || start > original.len() {
            anyhow::bail!("overlapping or out-of-range hunk in {}", patch.path);
        }
        output.extend(original[cursor..start].iter().cloned());
        cursor = start;
        for line in &hunk.lines {
            match line.kind {
                ' ' | '-' => {
                    let actual = original.get(cursor).ok_or_else(|| {
                        anyhow::anyhow!("patch context exceeds note: {}", patch.path)
                    })?;
                    if actual.text != line.line.text || actual.newline != line.line.newline {
                        anyhow::bail!(
                            "patch context does not match note: {} at line {}",
                            patch.path,
                            cursor + 1
                        );
                    }
                    if line.kind == ' ' {
                        output.push(actual.clone());
                    }
                    cursor += 1;
                }
                '+' => output.push(line.line.clone()),
                _ => unreachable!(),
            }
        }
        let added = hunk.lines.iter().filter(|line| line.kind != '-').count();
        if added != hunk.new_count {
            anyhow::bail!("invalid new hunk count for {}", patch.path);
        }
    }
    output.extend(original[cursor..].iter().cloned());
    let mut updated = String::new();
    for line in output {
        updated.push_str(&line.text);
        if line.newline {
            updated.push('\n');
        }
    }
    Ok(updated)
}
