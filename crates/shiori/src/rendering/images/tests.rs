use super::versioned;

#[test]
fn preview_versions_keep_original_encoding_queries_and_fragments() {
    for url in [
        "https://cms.test/prefix/assets/a%20b.svg",
        "a.png?x=1#caption",
        "a.png#fragment",
        "a.png?",
    ] {
        assert_eq!(versioned(url, None), url);
    }
    assert_eq!(
        versioned(
            "https://cms.test/prefix/assets/a%20b.svg?x=1#caption",
            Some("hash")
        ),
        "https://cms.test/prefix/assets/a%20b.svg?x=1&__watch=hash#caption"
    );
    assert_eq!(
        versioned("a.png#caption", Some("missing")),
        "a.png?__watch=missing#caption"
    );
}
