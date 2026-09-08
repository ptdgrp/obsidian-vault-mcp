use std::{fs, sync::Arc};

use camino::Utf8PathBuf;
use tempfile::tempdir;

use super::{IndexedNote, RefResolver, ResolveResult};
use crate::{
    parser::{NoteParser, ReferenceInfo},
    vault::{Vault, VaultConfig},
};

fn fixture() -> (tempfile::TempDir, Vec<IndexedNote>) {
    let dir = tempdir().expect("tempdir");
    fs::write(
        dir.path().join("林动.md"),
        "---\naliases:\n  - 动林\n---\n# 林动\n",
    )
    .expect("write note");
    fs::create_dir_all(dir.path().join("资料")).expect("资料 dir");
    fs::write(dir.path().join("资料/林动.md"), "# 资料林动\n").expect("write note");
    let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path");
    let vault = Vault::open(&root, VaultConfig::default()).expect("vault");
    let files = vault.list_notes().expect("list notes");

    let notes = files
        .into_iter()
        .map(|file| {
            let content = fs::read_to_string(&file.path).expect("read note");
            let parsed =
                Arc::new(NoteParser::parse(&file.relative_path, &content, 1024).expect("parse"));
            IndexedNote { file, parsed }
        })
        .collect::<Vec<_>>();
    (dir, notes)
}

#[test]
fn parse_ref_supports_embed_multi_heading_and_block_references() {
    let parsed = RefResolver::parse_ref("![[林动#设定#身体|查看]]");
    assert_eq!(parsed.target, "林动");
    assert_eq!(
        parsed.reference,
        Some(ReferenceInfo::MultiHeading {
            value: vec!["设定".to_string(), "身体".to_string()]
        })
    );

    let block = RefResolver::parse_ref("[[林动#^state]]");
    assert_eq!(
        block.reference,
        Some(ReferenceInfo::BlockId {
            value: "state".to_string()
        })
    );
}

#[test]
fn resolve_treats_missing_heading_selector_as_unresolved() {
    let dir = tempdir().expect("tempdir");
    fs::write(dir.path().join("Target.md"), "# Target\n\n## Present\n").expect("write note");
    let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path");
    let vault = Vault::open(&root, VaultConfig::default()).expect("vault");
    let file = vault
        .list_notes()
        .expect("list notes")
        .into_iter()
        .find(|file| file.relative_path == "Target.md")
        .expect("target file");
    let content = fs::read_to_string(&file.path).expect("read note");
    let parsed = Arc::new(NoteParser::parse(&file.relative_path, &content, 1024).expect("parse"));
    let notes = vec![IndexedNote { file, parsed }];

    let result = RefResolver::resolve("Target#Missing", &notes);

    match result {
        ResolveResult::Unresolved { reference } => {
            assert_eq!(reference.target, "Target");
        }
        other => panic!("expected missing heading to be unresolved, got {other:?}"),
    }
}

#[test]
fn resolve_accepts_level_one_heading_selector() {
    let dir = tempdir().expect("tempdir");
    fs::write(dir.path().join("Target.md"), "# Target\n\nBody\n").expect("write note");
    let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path");
    let vault = Vault::open(&root, VaultConfig::default()).expect("vault");
    let file = vault
        .list_notes()
        .expect("list notes")
        .into_iter()
        .find(|file| file.relative_path == "Target.md")
        .expect("target file");
    let content = fs::read_to_string(&file.path).expect("read note");
    let parsed = Arc::new(NoteParser::parse(&file.relative_path, &content, 1024).expect("parse"));
    let notes = vec![IndexedNote { file, parsed }];

    let result = RefResolver::resolve("Target#Target", &notes);

    match result {
        ResolveResult::Resolved {
            reference, heading, ..
        } => {
            assert_eq!(heading.as_deref(), Some("Target"));
            assert_eq!(
                reference.reference,
                Some(ReferenceInfo::Heading {
                    value: "Target".to_string()
                })
            );
        }
        other => panic!("expected level-one heading to resolve, got {other:?}"),
    }
}

#[test]
fn resolve_treats_missing_block_selector_as_unresolved() {
    let dir = tempdir().expect("tempdir");
    fs::write(dir.path().join("Target.md"), "# Target\n\n^present\n").expect("write note");
    let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path");
    let vault = Vault::open(&root, VaultConfig::default()).expect("vault");
    let file = vault
        .list_notes()
        .expect("list notes")
        .into_iter()
        .find(|file| file.relative_path == "Target.md")
        .expect("target file");
    let content = fs::read_to_string(&file.path).expect("read note");
    let parsed = Arc::new(NoteParser::parse(&file.relative_path, &content, 1024).expect("parse"));
    let notes = vec![IndexedNote { file, parsed }];

    let result = RefResolver::resolve("Target#^missing", &notes);

    match result {
        ResolveResult::Unresolved { reference } => {
            assert_eq!(reference.target, "Target");
        }
        other => panic!("expected missing block to be unresolved, got {other:?}"),
    }
}

#[test]
fn resolve_canonicalizes_slash_separated_heading_path_selector() {
    let dir = tempdir().expect("tempdir");
    fs::write(
        dir.path().join("Target.md"),
        "# Target\n\n## Parent\n\n### Child\n",
    )
    .expect("write note");
    let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path");
    let vault = Vault::open(&root, VaultConfig::default()).expect("vault");
    let file = vault
        .list_notes()
        .expect("list notes")
        .into_iter()
        .find(|file| file.relative_path == "Target.md")
        .expect("target file");
    let content = fs::read_to_string(&file.path).expect("read note");
    let parsed = Arc::new(NoteParser::parse(&file.relative_path, &content, 1024).expect("parse"));
    let notes = vec![IndexedNote { file, parsed }];

    let result = RefResolver::resolve("Target#Parent/Child", &notes);

    match result {
        ResolveResult::Resolved {
            reference, heading, ..
        } => {
            assert_eq!(heading.as_deref(), Some("Child"));
            assert_eq!(
                reference.reference,
                Some(ReferenceInfo::MultiHeading {
                    value: vec!["Parent".to_string(), "Child".to_string()]
                })
            );
        }
        other => panic!("expected resolved canonical heading path, got {other:?}"),
    }
}

#[test]
fn resolve_returns_ambiguous_candidates_for_duplicate_stems() {
    let (_dir, notes) = fixture();

    let result = RefResolver::resolve("林动", &notes);

    match result {
        ResolveResult::Ambiguous { candidates, .. } => {
            assert_eq!(candidates.len(), 2);
            assert!(
                candidates
                    .iter()
                    .any(|candidate| candidate.path == "林动.md")
            );
            assert!(
                candidates
                    .iter()
                    .any(|candidate| candidate.path == "资料/林动.md")
            );
        }
        other => panic!("expected ambiguous, got {other:?}"),
    }
}

#[test]
fn resolve_matches_numeric_prefix_stems_after_exact_stem() {
    let (dir, mut notes) = fixture();
    fs::write(dir.path().join("001-排序标题.md"), "# 排序标题\n").expect("write numbered note");
    let file = Vault::open(
        &Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path"),
        VaultConfig::default(),
    )
    .expect("vault")
    .list_notes()
    .expect("list notes")
    .into_iter()
    .find(|file| file.relative_path == "001-排序标题.md")
    .expect("numbered file");
    let content = fs::read_to_string(&file.path).expect("read note");
    let parsed = Arc::new(NoteParser::parse(&file.relative_path, &content, 1024).expect("parse"));
    notes.push(IndexedNote { file, parsed });

    let result = RefResolver::resolve("排序标题", &notes);

    match result {
        ResolveResult::Resolved { path, .. } => assert_eq!(path, "001-排序标题.md"),
        other => panic!("expected numbered-prefix resolution, got {other:?}"),
    }
}

#[test]
fn resolve_matches_named_numeric_sort_prefix_stems() {
    let (dir, mut notes) = fixture();
    for path in ["unit01-排序标题.md", "ch002-章节标题.md"] {
        fs::write(dir.path().join(path), "# 排序标题\n").expect("write prefixed note");
    }
    let vault = Vault::open(
        &Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path"),
        VaultConfig::default(),
    )
    .expect("vault");
    for file in vault
        .list_notes()
        .expect("list notes")
        .into_iter()
        .filter(|file| {
            file.relative_path.starts_with("unit") || file.relative_path.starts_with("ch")
        })
    {
        let content = fs::read_to_string(&file.path).expect("read note");
        let parsed =
            Arc::new(NoteParser::parse(&file.relative_path, &content, 1024).expect("parse"));
        notes.push(IndexedNote { file, parsed });
    }

    let unit = RefResolver::resolve("排序标题", &notes);
    let chapter = RefResolver::resolve("章节标题", &notes);

    match unit {
        ResolveResult::Resolved { path, .. } => assert_eq!(path, "unit01-排序标题.md"),
        other => panic!("expected unit-prefixed resolution, got {other:?}"),
    }
    match chapter {
        ResolveResult::Resolved { path, .. } => assert_eq!(path, "ch002-章节标题.md"),
        other => panic!("expected chapter-prefixed resolution, got {other:?}"),
    }
}

#[test]
fn resolve_does_not_strip_letter_only_prefix_stems() {
    let (dir, mut notes) = fixture();
    fs::write(dir.path().join("unit-排序标题.md"), "# 排序标题\n").expect("write prefixed note");
    let file = Vault::open(
        &Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path"),
        VaultConfig::default(),
    )
    .expect("vault")
    .list_notes()
    .expect("list notes")
    .into_iter()
    .find(|file| file.relative_path == "unit-排序标题.md")
    .expect("letter-only prefixed file");
    let content = fs::read_to_string(&file.path).expect("read note");
    let parsed = Arc::new(NoteParser::parse(&file.relative_path, &content, 1024).expect("parse"));
    notes.push(IndexedNote { file, parsed });

    let result = RefResolver::resolve("排序标题", &notes);

    match result {
        ResolveResult::Unresolved { .. } => {}
        other => panic!("expected letter-only prefix to stay unresolved, got {other:?}"),
    }
}

#[test]
fn resolve_reports_ambiguous_numeric_prefix_stems() {
    let (dir, mut notes) = fixture();
    for path in ["001-排序标题.md", "002-排序标题.md"] {
        fs::write(dir.path().join(path), "# 排序标题\n").expect("write numbered note");
    }
    let vault = Vault::open(
        &Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path"),
        VaultConfig::default(),
    )
    .expect("vault");
    for file in vault
        .list_notes()
        .expect("list notes")
        .into_iter()
        .filter(|file| file.relative_path.ends_with("-排序标题.md"))
    {
        let content = fs::read_to_string(&file.path).expect("read note");
        let parsed =
            Arc::new(NoteParser::parse(&file.relative_path, &content, 1024).expect("parse"));
        notes.push(IndexedNote { file, parsed });
    }

    let result = RefResolver::resolve("排序标题", &notes);

    match result {
        ResolveResult::Ambiguous { candidates, .. } => {
            assert_eq!(candidates.len(), 2);
            assert!(
                candidates
                    .iter()
                    .all(|candidate| candidate.match_kind == "numbered_stem")
            );
        }
        other => panic!("expected ambiguous numbered-prefix resolution, got {other:?}"),
    }
}

#[test]
fn resolve_prefers_exact_stem_over_numeric_prefix_stem() {
    let (dir, mut notes) = fixture();
    for path in ["排序标题.md", "001-排序标题.md"] {
        fs::write(dir.path().join(path), "# 排序标题\n").expect("write note");
    }
    let vault = Vault::open(
        &Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path"),
        VaultConfig::default(),
    )
    .expect("vault");
    for file in vault
        .list_notes()
        .expect("list notes")
        .into_iter()
        .filter(|file| {
            file.relative_path == "排序标题.md" || file.relative_path == "001-排序标题.md"
        })
    {
        let content = fs::read_to_string(&file.path).expect("read note");
        let parsed =
            Arc::new(NoteParser::parse(&file.relative_path, &content, 1024).expect("parse"));
        notes.push(IndexedNote { file, parsed });
    }

    let result = RefResolver::resolve("排序标题", &notes);

    match result {
        ResolveResult::Resolved { path, .. } => assert_eq!(path, "排序标题.md"),
        other => panic!("expected exact stem resolution, got {other:?}"),
    }
}

#[test]
fn resolve_does_not_apply_numeric_prefix_fallback_to_explicit_markdown_path() {
    let (dir, mut notes) = fixture();
    fs::write(dir.path().join("001-排序标题.md"), "# 排序标题\n").expect("write numbered note");
    let vault = Vault::open(
        &Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path"),
        VaultConfig::default(),
    )
    .expect("vault");
    let file = vault
        .list_notes()
        .expect("list notes")
        .into_iter()
        .find(|file| file.relative_path == "001-排序标题.md")
        .expect("numbered file");
    let content = fs::read_to_string(&file.path).expect("read note");
    let parsed = Arc::new(NoteParser::parse(&file.relative_path, &content, 1024).expect("parse"));
    notes.push(IndexedNote { file, parsed });

    let result = RefResolver::resolve("排序标题.md", &notes);

    match result {
        ResolveResult::Unresolved { .. } => {}
        other => panic!("expected explicit markdown path to stay unresolved, got {other:?}"),
    }
}
