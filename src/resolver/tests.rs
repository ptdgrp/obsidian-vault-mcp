use std::fs;

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
    let vault = Vault::open(root, VaultConfig::default()).expect("vault");
    let files = vault.list_notes().expect("list notes");

    let notes = files
        .into_iter()
        .map(|file| {
            let content = fs::read_to_string(&file.path).expect("read note");
            let parsed =
                NoteParser::parse(file.relative_path.clone(), &content, 1024).expect("parse");
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
fn link_matches_falls_back_to_normalized_path_comparison() {
    let (_dir, notes) = fixture();

    assert!(RefResolver::link_matches("林动", "林动.md", &notes));
    assert!(!RefResolver::link_matches("未知", "林动.md", &notes));
}
