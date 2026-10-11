//! CLI-only cwd paths. MCP inputs keep their workspace-relative semantics.
use anyhow::{Context, bail};
use camino::{Utf8Component, Utf8Path, Utf8PathBuf};

pub(super) fn cwd() -> anyhow::Result<Utf8PathBuf> {
    Utf8PathBuf::from_path_buf(std::env::current_dir()?)
        .map_err(|path| anyhow::anyhow!("current directory is not valid UTF-8: {}", path.display()))
}

pub(crate) fn absolute(path: &Utf8Path) -> anyhow::Result<Utf8PathBuf> {
    Ok(normalize(&if path.is_absolute() {
        path.to_owned()
    } else {
        cwd()?.join(path)
    }))
}

fn normalize(path: &Utf8Path) -> Utf8PathBuf {
    let mut out = Utf8PathBuf::new();
    for component in path.components() {
        match component {
            Utf8Component::CurDir => {}
            Utf8Component::ParentDir => {
                out.pop();
            }
            _ => out.push(component.as_str()),
        }
    }
    out
}

#[derive(Clone, Copy)]
pub(super) enum Kind {
    Reference,
    ExistingFile,
    Destination,
}

fn is_dot_path(path: &str) -> bool {
    path.starts_with("./")
        || path.starts_with("../")
        || (cfg!(windows) && (path.starts_with(".\\") || path.starts_with("..\\")))
}

/// Only reference fields have wikilink wrappers, selectors and display aliases.
/// Exact file fields may legitimately contain '#' or '|'.
pub(super) fn rebase(
    value: &mut String,
    root: &Utf8Path,
    cwd: &Utf8Path,
    kind: Kind,
) -> anyhow::Result<()> {
    let original = value.as_str();
    let (prefix, body, suffix) = if matches!(kind, Kind::Reference) {
        let trimmed = original.trim();
        if trimmed.starts_with("![[") && trimmed.ends_with("]]") {
            ("![[", &trimmed[3..trimmed.len() - 2], "]]")
        } else if trimmed.starts_with("[[") && trimmed.ends_with("]]") {
            ("[[", &trimmed[2..trimmed.len() - 2], "]]")
        } else {
            ("", trimmed, "")
        }
    } else {
        ("", original, "")
    };
    let target_end = if matches!(kind, Kind::Reference) {
        body.find(['#', '|']).unwrap_or(body.len())
    } else {
        body.len()
    };
    let target = if matches!(kind, Kind::Reference) {
        body[..target_end].trim()
    } else {
        &body[..target_end]
    };
    if !is_dot_path(target) {
        return Ok(());
    }

    let root = root.canonicalize_utf8()?;
    let cwd = cwd.canonicalize_utf8()?;
    let mut path = normalize(&cwd.join(target));
    if matches!(kind, Kind::Reference) && path.extension().is_none() {
        path.set_extension("md");
    }
    let relative = path
        .strip_prefix(&root)
        .with_context(|| format!("CLI path {target:?} escapes selected root {root}"))?;
    if matches!(kind, Kind::Destination) {
        // New files may not exist. Check the closest existing ancestor without
        // ignoring dangling symlinks, which must fail canonicalization.
        let mut ancestor = path.as_path();
        loop {
            match std::fs::symlink_metadata(ancestor) {
                Ok(_) => break,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    ancestor = ancestor
                        .parent()
                        .context("destination has no existing ancestor")?;
                }
                Err(error) => return Err(error.into()),
            }
        }
        let actual = ancestor
            .canonicalize_utf8()
            .with_context(|| format!("cannot resolve CLI destination {target:?}"))?;
        if !actual.starts_with(&root) {
            bail!("CLI destination {target:?} escapes selected root through a symlink");
        }
    } else {
        // Fail before reference resolution: a missing explicit cwd path must
        // never fall back to another note with the same filename or alias.
        let actual = path.canonicalize_utf8().with_context(|| {
            format!("CLI path does not exist or cannot be resolved: {target:?}")
        })?;
        if !actual.starts_with(&root) {
            bail!("CLI path {target:?} escapes selected root through a symlink");
        }
        if !actual.is_file() {
            bail!("CLI path is not a file: {target:?}");
        }
    }
    *value = format!(
        "{prefix}{}{rest}{suffix}",
        relative.as_str().replace('\\', "/"),
        rest = &body[target_end..]
    );
    Ok(())
}
