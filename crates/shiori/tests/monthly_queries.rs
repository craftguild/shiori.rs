mod common;
use common::*;
use serde_json::json;

#[tokio::test]
async fn utc_month_boundaries_use_git_creation_instead_of_names_or_modification_dates() {
    let root = tempfile::tempdir().unwrap();
    history::install(root.path(), "P");
    let (_, renderer, router) = history::app(root.path()).await;
    let q = "query($year:Int!,$month:Int!){postsByMonth(year:$year,month:$month){nodes{id createdAt updatedAt}totalCount}monthlyArchives{year month count}}";
    for (year, month, key, date, updated) in [
        (
            2020,
            1,
            "記事1",
            "2020-01-31T23:59:59.000Z",
            "2026-11-01T00:00:01.000Z",
        ),
        (
            2020,
            2,
            "foo/移動",
            "2020-02-29T23:59:59.000Z",
            "2020-02-29T23:59:59.000Z",
        ),
    ] {
        let result = query(&router, q, json!({"year":year,"month":month})).await;
        assert!(result.get("errors").is_none(), "{result}");
        assert_eq!(
            result["data"]["postsByMonth"]["nodes"],
            json!([{"id":key,"createdAt":date,"updatedAt":updated}])
        );
        assert_eq!(
            result["data"]["monthlyArchives"],
            json!([{"year":2026,"month":11,"count":25},{"year":2020,"month":2,"count":1},{"year":2020,"month":1,"count":1}])
        );
    }
    let absent = query(&router, q, json!({"year":2020,"month":3})).await;
    assert_eq!(absent["data"]["postsByMonth"]["totalCount"], 0);
    for (year, month) in [(0, 1), (10000, 1), (2026, 0), (2026, 13)] {
        let result = query(&router, q, json!({"year":year,"month":month})).await;
        assert_eq!(result["errors"][0]["extensions"]["code"], "BAD_USER_INPUT");
    }
    assert_eq!(renderer.calls(), 0);
}

#[tokio::test]
async fn calendar_boundaries_remain_inclusive_exclusive_across_the_supported_years() {
    let root = tempfile::tempdir().unwrap();
    history::install(root.path(), "T");
    let (_, renderer, router) = history::app(root.path()).await;
    for (year, month, expected) in [
        (1, 1, vec!["calendar/boundary-01", "calendar/boundary-00"]),
        (1, 2, vec!["calendar/boundary-02"]),
        (99, 12, vec!["calendar/boundary-03"]),
        (100, 1, vec!["calendar/boundary-04"]),
        (1969, 12, vec!["calendar/boundary-05"]),
        (1970, 1, vec!["calendar/boundary-06"]),
        (
            2023,
            2,
            vec!["calendar/boundary-08", "calendar/boundary-07"],
        ),
        (2023, 3, vec!["calendar/boundary-09"]),
        (
            2024,
            2,
            vec!["calendar/boundary-11", "calendar/boundary-10"],
        ),
        (2024, 3, vec!["calendar/boundary-12"]),
        (
            2026,
            12,
            vec!["calendar/boundary-14", "calendar/boundary-13"],
        ),
        (2027, 1, vec!["calendar/boundary-15"]),
        (9999, 11, vec!["calendar/boundary-16"]),
        (
            9999,
            12,
            vec!["calendar/boundary-18", "calendar/boundary-17"],
        ),
    ] {
        let result = query(&router, "query($year:Int!,$month:Int!){postsByMonth(year:$year,month:$month){nodes{id createdAt}totalCount}}", json!({"year":year,"month":month})).await;
        assert!(result.get("errors").is_none(), "{year}-{month}: {result}");
        let nodes = result["data"]["postsByMonth"]["nodes"].as_array().unwrap();
        assert_eq!(
            nodes
                .iter()
                .map(|post| post["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            expected,
            "{year}-{month}"
        );
        assert_eq!(result["data"]["postsByMonth"]["totalCount"], expected.len());
    }
    assert_eq!(renderer.calls(), 0);
}

#[tokio::test]
async fn monthly_nodes_themselves_resolve_bodies_and_required_render_resources() {
    let root = tempfile::tempdir().unwrap();
    history::install(root.path(), "T");
    let (_, renderer, router) = history::app(root.path()).await;
    let result = data(
        &router,
        "{postsByMonth(year:2024,month:2){nodes{id blocks{html}assets{kind mimeType}}totalCount}}",
    )
    .await;
    let posts = result["postsByMonth"]["nodes"].as_array().unwrap();
    assert_eq!(posts.len(), 2);
    for post in posts {
        let html = post["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|block| block["html"].as_str().unwrap())
            .collect::<String>();
        assert!(html.contains("<strong>body</strong>"));
        assert!(html.contains("<h1"));
        assert!(
            post["assets"]
                .as_array()
                .unwrap()
                .iter()
                .any(|asset| asset["mimeType"] == "font/otf")
        );
    }
    assert_eq!(renderer.calls(), 2);
}
