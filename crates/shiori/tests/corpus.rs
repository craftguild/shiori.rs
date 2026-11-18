mod common;
use common::*;
use serde_json::json;
use std::collections::BTreeSet;

// Deterministic serial correctness corpus: no timing, throughput or RSS gate.
#[tokio::test]
async fn five_thousand_article_serial_corpus() {
    const COUNT: usize = 5000;
    const BASE: i64 = 1_700_000_000_000;
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("assets")).unwrap();
    std::fs::write(
        root.path().join("assets/picture.svg"),
        "<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
    )
    .unwrap();
    let mut expected_tags = std::collections::BTreeMap::<String, usize>::new();
    for i in 0..COUNT {
        let tags = ["all".to_string(), format!("group-{}", i % 7)];
        for tag in &tags {
            *expected_tags.entry(tag.clone()).or_default() += 1;
        }
        let mut body = format!(
            "# Article {i}\n\nText **bold** [reference][r] and `code`.\n\n- item one\n- item two\n\n[r]: https://example.com/{i}\n\n"
        );
        if i % 100 == 0 {
            body.push_str(
                "![asset](/assets/picture.svg)\n\n```mermaid\nflowchart LR\n A --> B\n```\n\n",
            );
        }
        body.push_str(&"Corpus paragraph λ with *markup*. ".repeat(260));
        // Exact deterministic 8 KiB body; truncation is at a UTF-8 boundary.
        while body.len() > 8192 {
            body.pop();
        }
        while body.len() < 8192 {
            body.push(' ');
        }
        article(
            root.path(),
            BASE + i as i64,
            &format!("Article {i}"),
            &[&tags[0], &tags[1]],
            &body,
        );
    }
    let (catalog, renderer, router) = app(root.path()).await;
    assert_eq!(catalog.snapshot().articles.len(), COUNT);
    assert_eq!(catalog.snapshot().tags, expected_tags);
    let omitted = data(&router, "{ posts { nodes { id } totalCount } }").await;
    let explicit_null = data(
        &router,
        "{ posts(first:null, tag:null, after:null) { nodes { id } totalCount } }",
    )
    .await;
    assert_eq!(omitted, explicit_null);
    assert_eq!(omitted["posts"]["nodes"].as_array().unwrap().len(), 20);
    let query_text = "query($after: ID) { posts(first:100, after:$after) { nodes { id title tags createdAt } totalCount pageInfo { endCursor hasNextPage } } }";
    let mut after = serde_json::Value::Null;
    let mut ids = Vec::new();
    let mut previous: Option<(String, String)> = None;
    loop {
        let response = query(&router, query_text, json!({"after":after})).await;
        assert!(response.get("errors").is_none(), "{response}");
        let connection = &response["data"]["posts"];
        assert_eq!(connection["totalCount"], COUNT);
        for node in connection["nodes"].as_array().unwrap() {
            let id: i64 = node["id"].as_str().unwrap().parse().unwrap();
            let i = id - BASE;
            assert_eq!(node["title"], format!("Article {i}"));
            assert_eq!(node["tags"], json!(["all", format!("group-{}", i % 7)]));
            let key = node["id"].as_str().unwrap();
            let date = node["createdAt"].as_str().unwrap();
            if let Some((old_date, old_key)) = &previous {
                assert!(old_date.as_str() > date || (old_date == date && old_key.as_str() < key));
            }
            previous = Some((date.to_owned(), key.to_owned()));
            ids.push(id);
        }
        if connection["pageInfo"]["hasNextPage"] == false {
            break;
        }
        after = connection["pageInfo"]["endCursor"].clone();
    }
    assert_eq!(ids.len(), COUNT);
    assert_eq!(
        ids.iter().copied().collect::<BTreeSet<_>>(),
        (BASE..BASE + COUNT as i64).collect()
    );

    assert_eq!(renderer.calls(), 0);
    for i in [0, 1, 100, 2500, 4999] {
        let query_text = format!(
            "{{ post(id:\"{}\") {{ id title document {{ nodes {{ __typename ... on TextNode {{ text }} ... on ImageNode {{ presentation {{ url }} }} ... on FencedCodeNode {{ svg {{ markup }} }} }} }} }} }}",
            BASE + i
        );
        let first = data(&router, &query_text).await;
        assert!(first.to_string().contains(&format!("Article {i}")));
        let cached = data(&router, &query_text).await;
        assert_eq!(first, cached);
        if i % 100 == 0 {
            assert!(
                first
                    .to_string()
                    .contains("https://cms.example/blog/assets/picture.svg")
            );
        }
    }
    assert_eq!(renderer.calls(), 5);
    article(
        root.path(),
        BASE + 100,
        "Edited corpus",
        &["edited"],
        "# Edited body",
    );
    std::fs::remove_file(root.path().join(format!("articles/{}.md", BASE + 101))).unwrap();
    catalog.reconcile().await.unwrap();
    assert_eq!(catalog.snapshot().articles.len(), COUNT - 1);
    assert_eq!(catalog.snapshot().tags["all"], COUNT - 2);
    assert_eq!(catalog.snapshot().tags["edited"], 1);
    let edited = data(
        &router,
        &format!(
            "{{ post(id:\"{}\") {{ title document {{ nodes {{ ... on TextNode {{ text }} }} }} }} }}",
            BASE + 100
        ),
    )
    .await;
    assert_eq!(edited["post"]["title"], "Edited corpus");
    assert!(edited.to_string().contains("Edited body"));
    eprintln!(
        "corpus: count=5000 body_bytes=8192 ids={BASE}..{} tags=all+group-(i%7) asset+mermaid_every=100 revision=2 live_update=100 deletion=101; all serial assertions passed",
        BASE + 4999
    );
}
