mod common;

use common::*;
use serde_json::{Value, json};

const START: i64 = 1_793_491_200_000; // 2026-11-01T00:00:00Z

fn ids(connection: &Value) -> Vec<String> {
    connection["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["id"].as_str().unwrap().to_owned())
        .collect()
}

fn operation(filter: &str) -> String {
    let field = if filter.is_empty() { "posts" } else { filter };
    let arguments = if field == "posts" {
        ""
    } else if field == "postsByMonth" {
        "year:2026,month:11,"
    } else {
        "tag:\"Rust\","
    };
    let field = if field == "tag" { "posts" } else { field };
    format!(
        "query($first:Int,$after:ID,$before:ID) {{ {field}({arguments}first:$first,after:$after,before:$before) {{ nodes {{ id }} totalCount pageInfo {{ startCursor endCursor hasPreviousPage hasNextPage }} }} }}"
    )
}

#[tokio::test]
async fn five_item_roundtrips_and_nearest_newer_work_for_home_exact_tag_and_month() {
    let root = tempfile::tempdir().unwrap();
    for index in 1..=17 {
        article(
            root.path(),
            START + index * 2,
            "Article",
            &["Rust", "日本語 / %"],
            "Body",
        );
    }
    let (catalog, renderer, router) = app(root.path()).await;
    let all: Vec<_> = (1..=17)
        .rev()
        .map(|i| (START + i * 2).to_string())
        .collect();
    for field in ["posts", "tag", "postsByMonth"] {
        let query_text = operation(field);
        let key = if field == "postsByMonth" {
            field
        } else {
            "posts"
        };
        let mut pages = Vec::new();
        let mut after = Value::Null;
        for page in 0..4 {
            let response = query(&router, &query_text, json!({"first":5,"after":after})).await;
            assert!(response.get("errors").is_none(), "{response}");
            let connection = response["data"][key].clone();
            assert_eq!(ids(&connection), all[page * 5..(page * 5 + 5).min(17)]);
            assert_eq!(connection["totalCount"], 17);
            assert_eq!(connection["pageInfo"]["hasPreviousPage"], page > 0);
            assert_eq!(connection["pageInfo"]["hasNextPage"], page < 3);
            assert_eq!(connection["pageInfo"]["startCursor"], all[page * 5]);
            after = connection["pageInfo"]["endCursor"].clone();
            pages.push(connection);
        }
        for page in (1..4).rev() {
            let response = query(
                &router,
                &query_text,
                json!({"first":5,"before":pages[page]["pageInfo"]["startCursor"]}),
            )
            .await;
            assert_eq!(response["data"][key], pages[page - 1]);
        }
        let response = query(
            &router,
            &query_text,
            json!({"first":5,"before":(START+13).to_string()}),
        )
        .await;
        assert_eq!(ids(&response["data"][key]), all[6..11]);
        for variables in [
            json!({"first":5,"after":(START+2).to_string()}),
            json!({"first":5,"before":(START+34).to_string()}),
        ] {
            let response = query(&router, &query_text, variables).await;
            assert_eq!(
                response["data"][key],
                json!({"nodes":[],"totalCount":17,"pageInfo":{"startCursor":null,"endCursor":null,"hasPreviousPage":false,"hasNextPage":false}})
            );
        }
    }
    std::fs::remove_file(root.path().join(format!("contents/{}.md", START + 14))).unwrap();
    catalog.reconcile().await.unwrap();
    let response = query(
        &router,
        &operation("posts"),
        json!({"first":5,"before":(START+14).to_string()}),
    )
    .await;
    assert_eq!(ids(&response["data"]["posts"]), all[5..10]);
    assert_eq!(response["data"]["posts"]["totalCount"], 16);
    assert_eq!(renderer.calls(), 0);
    assert_eq!(renderer.images.inspections(), 0);
}

#[tokio::test]
async fn cursors_validate_and_null_or_omitted_inputs_preserve_twenty_default() {
    let root = tempfile::tempdir().unwrap();
    for id in 1..=25 {
        article(root.path(), id, "Article", &[], "Body");
    }
    let (_, _, router) = app(root.path()).await;
    for field in ["posts", "postsByMonth"] {
        let text = operation(field).replace("year:2026,month:11", "year:1970,month:1");
        for vars in [json!({}), json!({"first":null,"after":null,"before":null})] {
            let response = query(&router, &text, vars).await;
            assert!(response.get("errors").is_none(), "{response}");
            assert_eq!(ids(&response["data"][field]).len(), 20);
        }
        for vars in [
            json!({"after":"5","before":"6"}),
            json!({"before":"0"}),
            json!({"before":"01"}),
            json!({"before":"+1"}),
            json!({"before":"1.0"}),
            json!({"before":" 1"}),
            json!({"before":"253402300800000"}),
            json!({"before":"9223372036854775807"}),
            json!({"first":0}),
            json!({"first":101}),
        ] {
            let response = query(&router, &text, vars).await;
            assert_eq!(
                response["errors"][0]["extensions"]["code"], "BAD_USER_INPUT",
                "{response}"
            );
        }
        for vars in [
            json!({"after":null,"before":"10","first":5}),
            json!({"before":null,"after":"16","first":5}),
        ] {
            let response = query(&router, &text, vars).await;
            assert!(response.get("errors").is_none(), "{response}");
            assert_eq!(
                ids(&response["data"][field]),
                ["15", "14", "13", "12", "11"]
            );
        }
    }
    let value = data(&router, "{ posts(first:5,before:\"6\") { nodes { id } } }").await;
    assert_eq!(ids(&value["posts"]), ["11", "10", "9", "8", "7"]);
}

#[tokio::test]
async fn archives_are_populated_utc_global_and_metadata_only_with_filtered_flags() {
    let root = tempfile::tempdir().unwrap();
    article(root.path(), 1, "Epoch", &["Rust"], "Body");
    article(root.path(), START - 1, "October UTC", &["Rust"], "Body");
    article(root.path(), START, "November UTC", &["rust"], "Body");
    article(root.path(), START + 1, "November UTC", &["Rust"], "Body");
    article(root.path(), 253_402_300_799_999, "Last UTC", &[], "Body");
    let (_, renderer, router) = app(root.path()).await;
    let value=data(&router,"{ monthlyArchives { year month count } tags { name count } posts(tag:\"rust\",first:5,before:\"253402300799999\") { nodes { id } totalCount pageInfo { hasNextPage hasPreviousPage } } }").await;
    assert_eq!(
        value["monthlyArchives"],
        json!([{"year":9999,"month":12,"count":1},{"year":2026,"month":11,"count":2},{"year":2026,"month":10,"count":1},{"year":1970,"month":1,"count":1}])
    );
    assert_eq!(
        value["tags"],
        json!([{"name":"Rust","count":3},{"name":"rust","count":1}])
    );
    assert_eq!(value["posts"]["totalCount"], 1);
    assert_eq!(value["posts"]["nodes"], json!([]));
    assert_eq!(
        value["posts"]["pageInfo"],
        json!({"hasNextPage":false,"hasPreviousPage":false})
    );
    let value=data(&router,&format!("{{ posts(tag:\"Rust\", first:1, before:\"{}\") {{ nodes {{ id }} pageInfo {{ hasPreviousPage hasNextPage }} }} }}",START)).await;
    assert_eq!(ids(&value["posts"]), [(START + 1).to_string()]);
    assert_eq!(
        value["posts"]["pageInfo"],
        json!({"hasPreviousPage":false,"hasNextPage":true})
    );
    assert_eq!(renderer.calls(), 0);
    assert_eq!(renderer.images.inspections(), 0);
    let empty = tempfile::tempdir().unwrap();
    std::fs::create_dir(empty.path().join("contents")).unwrap();
    let (_, _, router) = app(empty.path()).await;
    assert_eq!(
        data(&router, "{ monthlyArchives { year } }").await,
        json!({"monthlyArchives":[]})
    );
}
