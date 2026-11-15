mod common;
use common::*;
use serde_json::{Value, json};

fn ids(value: &Value) -> Vec<String> {
    value["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn date_order_and_nearest_newer_roundtrip_for_global_tag_month_and_search() {
    let root = tempfile::tempdir().unwrap();
    history::install(root.path(), "P");
    let (catalog, renderer, router) = history::app(root.path()).await;
    for field in [
        "posts",
        "posts(tag:\"Page\",",
        "posts(query:\"searchable\",",
        "postsByMonth(year:2026,month:11,",
    ] {
        let (name, args) = field.split_once('(').unwrap_or((field, ""));
        let q = format!(
            "query($after:ID,$before:ID) {{ {name}({args}first:5,after:$after,before:$before) {{ nodes {{ id }} totalCount pageInfo {{startCursor endCursor hasPreviousPage hasNextPage}} }} }}"
        );
        let mut pages = Vec::new();
        let mut after = Value::Null;
        let count = if field == "posts" { 27 } else { 25 };
        for offset in (0..count).step_by(5) {
            let result = query(&router, &q, json!({"after":after})).await;
            assert!(result.get("errors").is_none(), "{result}");
            let page = result["data"][name].clone();
            assert_eq!(page["totalCount"], count);
            assert_eq!(page["pageInfo"]["hasPreviousPage"], offset > 0);
            assert_eq!(page["pageInfo"]["hasNextPage"], offset + 5 < count);
            let expected: Vec<_> = catalog
                .snapshot()
                .ordered
                .iter()
                .filter(|p| field == "posts" || p.metadata.tags.iter().any(|t| t == "Page"))
                .skip(offset)
                .take(5)
                .map(|p| p.id.clone())
                .collect();
            assert_eq!(ids(&page), expected);
            assert!(
                page["pageInfo"]["startCursor"]
                    .as_str()
                    .unwrap()
                    .starts_with("v1.")
            );
            after = page["pageInfo"]["endCursor"].clone();
            pages.push(page);
        }
        for index in (1..pages.len()).rev() {
            let result = query(
                &router,
                &q,
                json!({"before":pages[index]["pageInfo"]["startCursor"]}),
            )
            .await;
            assert_eq!(result["data"][name], pages[index - 1]);
        }
    }
    let q = "query($after:ID){ posts(first:5,tag:\"Page\",after:$after){nodes{id}totalCount pageInfo{endCursor}} }";
    let first = query(&router, q, json!({})).await;
    let cursor = first["data"]["posts"]["pageInfo"]["endCursor"].clone();
    let deleted = first["data"]["posts"]["nodes"][4]["id"].as_str().unwrap();
    std::fs::remove_file(root.path().join(format!("site/articles/{deleted}.md"))).unwrap();
    catalog.reconcile().await.unwrap();
    let next = query(&router, q, json!({"after":cursor})).await;
    assert_eq!(
        ids(&next["data"]["posts"]),
        (16..=20)
            .rev()
            .map(|i| format!("page/記事{i:02}"))
            .collect::<Vec<_>>()
    );
    assert_eq!(next["data"]["posts"]["totalCount"], 24);
    assert_eq!(renderer.calls(), 0);
    assert_eq!(renderer.images.inspections(), 0);
}

#[tokio::test]
async fn numeric_or_malformed_cursors_are_rejected_and_null_defaults_still_work() {
    let root = tempfile::tempdir().unwrap();
    history::install(root.path(), "P");
    let (_, _, router) = history::app(root.path()).await;
    let q = "query($after:ID,$before:ID,$first:Int){posts(after:$after,before:$before,first:$first){nodes{id}totalCount}}";
    for args in [json!({}), json!({"after":null,"before":null,"first":null})] {
        let result = query(&router, q, args).await;
        assert_eq!(ids(&result["data"]["posts"]).len(), 20);
    }
    for args in [
        json!({"after":"1"}),
        json!({"before":"v1.bad"}),
        json!({"after":"1","before":"2"}),
        json!({"first":0}),
        json!({"first":101}),
    ] {
        let result = query(&router, q, args).await;
        assert_eq!(result["errors"][0]["extensions"]["code"], "BAD_USER_INPUT");
    }
}
