use shiori::{
    generate::{generate, generate_with_clock},
    metadata::{self, MAX_FILE_BYTES},
    storage,
};

#[test]
fn strict_metadata_unicode_crlf_and_identity() {
    let bytes = "---\r\ntitle: '  Hello λ  '\r\ndescription: '  preserved  '\r\ntags: [' Rust ', '', Rust, rust, '日本語']\r\nextra: {any: [1, true]}\r\n---\r\nBody\r\n";
    let article = metadata::parse(1, bytes.as_bytes()).unwrap();
    assert_eq!(article.id, 1);
    assert_eq!(article.created_at, "1970-01-01T00:00:00.001Z");
    assert_eq!(article.metadata.title, "Hello λ");
    assert_eq!(article.metadata.description, "  preserved  ");
    assert_eq!(article.metadata.tags, ["Rust", "rust", "日本語"]);
    assert_eq!(article.body, "Body\r\n");
    let edited = metadata::parse(1, b"---\ntitle: Changed\n---\nNew body").unwrap();
    assert_eq!(article.created_at, edited.created_at);
    assert_ne!(article.hash, edited.hash);
}

#[test]
fn malformed_metadata_and_bounds_fail() {
    for yaml in [
        "title: a\ntitle: b",
        "title: 123",
        "title: true",
        "title: null",
        "title: []",
        "title: ' '",
        "description: 1\ntitle: a",
        "title: a\ntags: null",
        "title: a\ntags: [1]",
        "title: a\nextra: {x: 1, x: 2}",
        "title: [",
        "- a",
    ] {
        assert!(
            metadata::parse(1, format!("---\n{yaml}\n---\n").as_bytes()).is_err(),
            "{yaml}"
        );
    }
    for id in [
        "",
        "0",
        "01",
        "-1",
        "+1",
        "1.5",
        "１",
        "9999999999999999999",
        "253402300800000",
    ] {
        assert!(metadata::parse_id(id).is_err(), "{id}");
    }
    assert_eq!(metadata::parse_id("9").unwrap(), 9);
    assert!(metadata::parse(1, &vec![b'a'; MAX_FILE_BYTES + 1]).is_err());
    assert!(metadata::parse(1, b"---\ntitle: \xff\n---\n").is_err());
    let deep = format!(
        "---\ntitle: ok\nunknown: {}0{}\n---\n",
        "[".repeat(100),
        "]".repeat(100)
    );
    assert!(metadata::parse(1, deep.as_bytes()).is_err());
    let empty = metadata::parse(1, b"---\ntitle: ok\n---\n").unwrap();
    assert!(empty.body.is_empty());
    assert!(empty.metadata.tags.is_empty());
    // Unknown metadata may contain YAML-native keys/values; known fields stay strict.
    let unknown = b"---\ntitle: ok\nextra: {42: .nan, true: null}\n---\n";
    assert!(
        metadata::parse(1, unknown).is_ok(),
        "{:?}",
        metadata::parse(1, unknown)
    );
}

#[test]
fn generated_yaml_is_valid_no_clobber_and_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let existing = root.path().join("contents/100.md");
    std::fs::create_dir(root.path().join("contents")).unwrap();
    std::fs::write(&existing, "original").unwrap();
    let mut ticks = ["100", "100", "101"].into_iter();
    let path = generate_with_clock(root.path(), " 'λ':\nNew title ", || {
        Ok(ticks.next().unwrap().into())
    })
    .unwrap();
    assert_eq!(path.file_name().unwrap(), "101.md");
    assert!(root.path().join("assets/101").is_dir());
    assert!(!root.path().join("assets/100").exists());
    assert_eq!(std::fs::read_to_string(existing).unwrap(), "original");
    let parsed = metadata::parse(101, &std::fs::read(path).unwrap()).unwrap();
    assert_eq!(parsed.metadata.title, "'λ':\nNew title");
    assert_eq!(parsed.body, "\n");
    assert!(parsed.metadata.tags.is_empty());
    let error = generate_with_clock(root.path(), "ok", || anyhow::bail!("clock failed"));
    assert!(error.unwrap_err().to_string().contains("clock failed"));
    assert_eq!(
        std::fs::read_dir(root.path().join("contents"))
            .unwrap()
            .count(),
        2
    );
    assert!(generate(root.path(), " ").is_err());
    assert!(generate(&root.path().join("missing"), "ok").is_err());
    // An actual stuck-clock collision reaches its bounded retry failure without
    // overwriting source or leaving its temporary file behind.
    let collision = generate_with_clock(root.path(), "Collision", || Ok("100".into()));
    assert!(collision.unwrap_err().to_string().contains("one second"));
    assert_eq!(
        std::fs::read_dir(root.path().join("contents"))
            .unwrap()
            .count(),
        2
    );
    assert!(generate(root.path(), &"\0".repeat(32 * 1024)).is_err());
}

#[test]
fn concurrent_generation_and_filesystem_errors() {
    let root = tempfile::tempdir().unwrap();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| scope.spawn(|| generate(root.path(), "Concurrent").unwrap()))
            .collect();
        let paths: std::collections::HashSet<_> =
            handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(paths.len(), 8);
        for path in paths {
            assert!(
                root.path()
                    .join("assets")
                    .join(path.file_stem().unwrap())
                    .is_dir()
            );
            let id = metadata::parse_id(path.file_stem().unwrap().to_str().unwrap()).unwrap();
            assert_eq!(
                metadata::parse(id, &std::fs::read(path).unwrap())
                    .unwrap()
                    .metadata
                    .title,
                "Concurrent"
            );
        }
    });
    assert_eq!(
        std::fs::read_dir(root.path().join("contents"))
            .unwrap()
            .count(),
        8
    );
    let outside = tempfile::tempdir().unwrap();
    let bad = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), bad.path().join("contents")).unwrap();
    assert!(generate(bad.path(), "No escape").is_err());
    let fd = storage::root(bad.path()).unwrap();
    assert!(storage::directory(&fd, "contents").is_err());
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
}

#[test]
fn asset_reservations_preserve_existing_files_and_reject_unsafe_or_unwritable_roots() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("assets/100")).unwrap();
    std::fs::write(root.path().join("assets/100/photo.png"), b"preserved").unwrap();
    let mut clock = ["100", "101"].into_iter();
    let path =
        generate_with_clock(root.path(), "New", || Ok(clock.next().unwrap().into())).unwrap();
    assert_eq!(path.file_name().unwrap(), "101.md");
    assert_eq!(
        std::fs::read(root.path().join("assets/100/photo.png")).unwrap(),
        b"preserved"
    );
    assert!(!root.path().join("contents/100.md").exists());
    let outside = tempfile::tempdir().unwrap();
    let unsafe_root = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), unsafe_root.path().join("assets")).unwrap();
    assert!(generate_with_clock(unsafe_root.path(), "Unsafe", || Ok("1".into())).is_err());
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    assert_eq!(
        std::fs::read_dir(unsafe_root.path().join("contents"))
            .unwrap()
            .count(),
        0
    );
    let occupied = tempfile::tempdir().unwrap();
    std::fs::write(occupied.path().join("assets"), b"not a directory").unwrap();
    assert!(generate(occupied.path(), "Unsafe").is_err());
    assert_eq!(
        std::fs::read_dir(occupied.path().join("contents"))
            .unwrap()
            .count(),
        0
    );
    let assets = root.path().join("assets");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&assets, std::fs::Permissions::from_mode(0o555)).unwrap();
    let result = generate_with_clock(root.path(), "Denied", || Ok("102".into()));
    std::fs::set_permissions(&assets, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(result.is_err());
    assert!(!root.path().join("contents/102.md").exists());
    assert!(!assets.join("102").exists());
}
