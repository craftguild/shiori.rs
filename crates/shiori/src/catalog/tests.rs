use super::*;

#[test]
fn prepared_scan_cannot_publish_a_revision_that_changed_while_queued() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("articles")).unwrap();
    let file = root.path().join("articles/1.md");
    std::fs::write(&file, "---\ntitle: Old\n---\nBody").unwrap();
    std::fs::create_dir_all(root.path().join("assets")).unwrap();
    let fd = storage::root(root.path()).unwrap();
    let pending = prepare_scan(&fd, None).unwrap();
    std::fs::write(file, "---\ntitle: New\n---\nBody").unwrap();
    assert!(pending.finish(&fd).is_err());
    assert_eq!(scan(&fd, None).unwrap().articles["1"].metadata.title, "New");
}

#[test]
fn repeated_extension_is_a_valid_key_and_nonregular_files_are_ignored() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("articles")).unwrap();
    std::fs::write(
        root.path().join("articles/1.md.md"),
        "---\ntitle: Invalid\n---\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.path().join("assets")).unwrap();
    let fd = storage::root(root.path()).unwrap();
    assert!(scan(&fd, None).unwrap().articles.contains_key("1.md"));
    std::fs::remove_file(root.path().join("articles/1.md.md")).unwrap();
    std::fs::create_dir(root.path().join("articles/2.md")).unwrap();
    std::os::unix::fs::symlink("outside", root.path().join("articles/3.md")).unwrap();
    assert!(scan(&fd, None).unwrap().articles.is_empty());
}
