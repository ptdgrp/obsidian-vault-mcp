use std::fs;

use camino::Utf8PathBuf;
use tempfile::tempdir;

use super::{Vault, VaultConfig, VaultError};

fn fixture(config: VaultConfig) -> (tempfile::TempDir, Vault) {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path");
    let vault = Vault::open(root, config).expect("vault");
    (dir, vault)
}

#[test]
fn open_rejects_missing_or_file_roots() {
    let dir = tempdir().expect("tempdir");
    let missing = Utf8PathBuf::from_path_buf(dir.path().join("missing")).expect("utf8 path");
    let error =
        Vault::open(missing.clone(), VaultConfig::default()).expect_err("missing root should fail");
    assert!(matches!(error, VaultError::RootIsNotDirectory(path) if path == missing.to_string()));

    let file = dir.path().join("vault.md");
    fs::write(&file, "# not a directory\n").expect("write file root");
    let file = Utf8PathBuf::from_path_buf(file).expect("utf8 path");
    let error =
        Vault::open(file.clone(), VaultConfig::default()).expect_err("file root should fail");
    assert!(matches!(error, VaultError::RootIsNotDirectory(path) if path == file.to_string()));
}

#[test]
fn read_note_appends_markdown_extension_and_enforces_size_limit() {
    let (dir, vault) = fixture(VaultConfig {
        max_note_bytes: 4,
        ..VaultConfig::default()
    });
    fs::write(dir.path().join("短.md"), "1234").expect("write short note");
    fs::write(dir.path().join("长.md"), "12345").expect("write long note");

    let (_, content) = vault.read_note("短").expect("read short note");
    assert_eq!(content, "1234");

    let error = vault.read_note("长").expect_err("large note should fail");
    assert!(matches!(
        error,
        VaultError::NoteTooLarge {
            path,
            limit: 4,
            actual: 5
        } if path == "长.md"
    ));
}

#[test]
fn write_note_atomic_rejects_outside_paths_and_persists_inside_vault() {
    let (dir, vault) = fixture(VaultConfig::default());
    let inside = vault.resolve_path("笔记.md").expect("resolve inside");
    vault
        .write_note_atomic(&inside, "# 笔记\n")
        .expect("write inside");
    assert_eq!(
        fs::read_to_string(dir.path().join("笔记.md")).expect("read inside"),
        "# 笔记\n"
    );

    let outside = Utf8PathBuf::from("/tmp/outside.md");
    let error = vault
        .write_note_atomic(&outside, "# 外部\n")
        .expect_err("outside path should fail");
    assert!(matches!(error, VaultError::PathEscapesVault));
}

#[test]
fn write_note_atomic_reports_missing_parent_directory() {
    let (_dir, vault) = fixture(VaultConfig::default());
    let missing_parent = vault
        .resolve_path("missing/笔记.md")
        .expect("resolve missing parent path");

    let error = vault
        .write_note_atomic(&missing_parent, "# 笔记\n")
        .expect_err("missing parent should fail");

    assert!(matches!(error, VaultError::Io(_)));
}

#[test]
fn list_notes_honors_include_globs_and_reports_invalid_patterns() {
    let (dir, mut vault) = fixture(VaultConfig::default());
    fs::create_dir_all(dir.path().join("正文")).expect("chapter dir");
    fs::create_dir_all(dir.path().join("设定")).expect("setting dir");
    fs::write(dir.path().join("正文/001.md"), "# 第一章\n").expect("write chapter");
    fs::write(dir.path().join("设定/术语.md"), "# 术语\n").expect("write setting");

    vault.config.include = vec!["正文/**/*.md".to_string()];
    let notes = vault.list_notes().expect("list notes");
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].relative_path, "正文/001.md");

    vault.config.include = vec!["[".to_string()];
    let error = vault.list_notes().expect_err("invalid glob should fail");
    assert!(matches!(error, VaultError::InvalidGlob(_)));
}

#[test]
fn resolve_path_rejects_absolute_paths_and_relative_path_uses_vault_relative_form() {
    let (dir, vault) = fixture(VaultConfig::default());

    let error = vault
        .resolve_path("/tmp/absolute.md")
        .expect_err("absolute path should fail");
    assert!(matches!(error, VaultError::AbsolutePathNotAllowed));

    let nested = dir.path().join("正文/001.md");
    fs::create_dir_all(nested.parent().expect("parent")).expect("create dir");
    fs::write(&nested, "# 第一章\n").expect("write nested note");
    let nested = Utf8PathBuf::from_path_buf(nested).expect("utf8 path");
    assert_eq!(vault.relative_path(&nested), "正文/001.md");
}

#[test]
fn resolve_exact_note_path_requires_safe_markdown_paths() {
    let (dir, vault) = fixture(VaultConfig::default());
    fs::write(dir.path().join("人物.md"), "# 人物\n").expect("write note");

    assert!(vault.resolve_exact_note_path("人物.md").is_ok());
    assert!(vault.resolve_exact_note_path("人物").is_err());
    assert!(vault.resolve_exact_note_path("/tmp/人物.md").is_err());
    assert!(vault.resolve_exact_note_path("../人物.md").is_err());
}

#[test]
fn resolve_path_normalizes_dot_segments_and_rejects_parent_escape() {
    let (_dir, vault) = fixture(VaultConfig::default());

    let normalized = vault
        .resolve_path("正文/../设定/./术语.md")
        .expect("normalize path");
    assert_eq!(vault.relative_path(&normalized), "设定/术语.md");

    let error = vault
        .resolve_path("正文/../../outside.md")
        .expect_err("parent traversal should fail");
    assert!(matches!(error, VaultError::PathEscapesVault));
}

#[test]
fn list_notes_honors_exclude_globs_and_natural_sorting() {
    let (dir, mut vault) = fixture(VaultConfig::default());
    fs::write(dir.path().join("10.md"), "# ten\n").expect("write ten");
    fs::write(dir.path().join("2.md"), "# two\n").expect("write two");
    fs::write(dir.path().join("skip.md"), "# skip\n").expect("write skip");

    vault.config.exclude = vec!["skip.md".to_string()];
    let notes = vault.list_notes().expect("list notes");
    let paths = notes
        .into_iter()
        .map(|note| note.relative_path)
        .collect::<Vec<_>>();

    assert_eq!(paths, vec!["2.md".to_string(), "10.md".to_string()]);
}

#[test]
fn list_notes_ignores_default_hidden_and_generated_paths() {
    let (dir, vault) = fixture(VaultConfig::default());
    fs::write(dir.path().join("visible.md"), "# visible\n").expect("write visible");
    fs::write(dir.path().join("attachment.txt"), "not markdown").expect("write non-md");
    fs::write(dir.path().join(".hidden.md"), "# hidden\n").expect("write hidden");
    fs::create_dir_all(dir.path().join("target")).expect("target dir");
    fs::write(dir.path().join("target/build.md"), "# build\n").expect("write target");
    fs::create_dir_all(dir.path().join("nested/target")).expect("nested target dir");
    fs::write(dir.path().join("nested/target/build.md"), "# build\n").expect("write nested target");
    fs::create_dir_all(dir.path().join("nested/.cache")).expect("cache dir");
    fs::write(dir.path().join("nested/.cache/cache.md"), "# cache\n").expect("write cache");
    fs::create_dir_all(dir.path().join("nested/node_modules")).expect("node_modules dir");
    fs::write(dir.path().join("nested/node_modules/pkg.md"), "# pkg\n").expect("write package");

    let notes = vault.list_notes().expect("list notes");
    let paths = notes
        .into_iter()
        .map(|note| note.relative_path)
        .collect::<Vec<_>>();

    assert_eq!(paths, vec!["visible.md".to_string()]);
}
