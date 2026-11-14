mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::*;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use time::{OffsetDateTime, macros::datetime};
use tower::ServiceExt;

const MONTH_QUERY: &str = "query($year: Int!, $month: Int!, $first: Int, $after: ID) {
  postsByMonth(year: $year, month: $month, first: $first, after: $after) {
    nodes { id title createdAt tags }
    totalCount
    pageInfo { endCursor hasNextPage }
  }
}";

fn millis(value: OffsetDateTime) -> i64 {
    (value.unix_timestamp_nanos() / 1_000_000)
        .try_into()
        .unwrap()
}

fn ids(connection: &Value) -> Vec<String> {
    connection["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["id"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn utc_month_boundaries_cover_leap_years_year_end_and_timestamp_extremes() {
    let maximum = millis(datetime!(9999-12-31 23:59:59.999 UTC));
    for (year, month, start, end) in [
        (
            1970,
            1,
            millis(datetime!(1970-01-01 00:00 UTC)),
            millis(datetime!(1970-02-01 00:00 UTC)),
        ),
        (
            2023,
            2,
            millis(datetime!(2023-02-01 00:00 UTC)),
            millis(datetime!(2023-03-01 00:00 UTC)),
        ),
        (
            2024,
            2,
            millis(datetime!(2024-02-01 00:00 UTC)),
            millis(datetime!(2024-03-01 00:00 UTC)),
        ),
        (
            2026,
            11,
            millis(datetime!(2026-11-01 00:00 UTC)),
            millis(datetime!(2026-12-01 00:00 UTC)),
        ),
        (
            2026,
            12,
            millis(datetime!(2026-12-01 00:00 UTC)),
            millis(datetime!(2027-01-01 00:00 UTC)),
        ),
        (
            9999,
            12,
            millis(datetime!(9999-12-01 00:00 UTC)),
            maximum + 1,
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let first = start.max(1);
        for id in [first, first + 1, end - 1] {
            article(root.path(), id, "Included", &[], "Body");
        }
        if start > 1 {
            article(root.path(), start - 1, "Before month", &[], "Body");
        }
        if end <= maximum {
            article(root.path(), end, "Next month", &[], "Body");
        }
        let (_, renderer, router) = app(root.path()).await;
        let response = query(&router, MONTH_QUERY, json!({"year":year,"month":month})).await;
        assert!(response.get("errors").is_none(), "{response}");
        let connection = &response["data"]["postsByMonth"];
        assert_eq!(
            ids(connection),
            [end - 1, first + 1, first].map(|id| id.to_string()),
            "{year}-{month}"
        );
        assert_eq!(connection["totalCount"], 3);
        assert_eq!(
            connection["pageInfo"],
            json!({"endCursor":first.to_string(),"hasNextPage":false})
        );
        assert_eq!(renderer.calls(), 0);
    }
}

#[tokio::test]
async fn month_membership_and_returned_creation_time_use_utc() {
    let root = tempfile::tempdir().unwrap();
    let october = millis(datetime!(2026-10-31 15:00 UTC));
    let november = millis(datetime!(2026-11-30 15:00 UTC));
    article(root.path(), october, "November in JST", &[], "Body");
    article(root.path(), november, "December in JST", &[], "Body");
    let (_, _, router) = app(root.path()).await;
    for (month, id, created_at) in [
        (10, october, "2026-10-31T15:00:00.000Z"),
        (11, november, "2026-11-30T15:00:00.000Z"),
    ] {
        let response = query(&router, MONTH_QUERY, json!({"year":2026,"month":month})).await;
        assert!(response.get("errors").is_none(), "{response}");
        let connection = &response["data"]["postsByMonth"];
        assert_eq!(ids(connection), [id.to_string()]);
        assert_eq!(connection["nodes"][0]["createdAt"], created_at);
    }
}

#[tokio::test]
async fn monthly_pagination_counts_defaults_and_deleted_or_outside_cursors() {
    let root = tempfile::tempdir().unwrap();
    let start = millis(datetime!(2026-11-01 00:00 UTC));
    let end = millis(datetime!(2026-12-01 00:00 UTC));
    for index in 0..25 {
        article(
            root.path(),
            start + 2 * (index + 1),
            "Monthly",
            if index % 2 == 0 { &["Rust"] } else { &[] },
            "Body",
        );
    }
    article(root.path(), start - 1, "Older", &[], "Body");
    article(root.path(), end, "Newer", &[], "Body");
    let (catalog, renderer, router) = app(root.path()).await;
    let first = data(&router, "{ postsByMonth(year:2026, month:11) { nodes { id } totalCount pageInfo { endCursor hasNextPage } } }").await;
    let expected: Vec<_> = (1..=25)
        .rev()
        .map(|index| (start + 2 * index).to_string())
        .collect();
    let first = &first["postsByMonth"];
    assert_eq!(ids(first), expected[..20]);
    assert_eq!(first["totalCount"], 25);
    assert_eq!(
        first["pageInfo"],
        json!({"endCursor":expected[19],"hasNextPage":true})
    );
    for variables in [
        json!({"year":2026,"month":11,"first":null,"after":null}),
        json!({"year":2026,"month":11,"after":end.to_string()}),
    ] {
        let response = query(&router, MONTH_QUERY, variables).await;
        assert!(response.get("errors").is_none(), "{response}");
        assert_eq!(ids(&response["data"]["postsByMonth"]), expected[..20]);
        assert_eq!(response["data"]["postsByMonth"]["totalCount"], 25);
    }
    for (first, count, has_next) in [(1, 1, true), (100, 25, false)] {
        let response = query(
            &router,
            MONTH_QUERY,
            json!({"year":2026,"month":11,"first":first}),
        )
        .await;
        assert!(response.get("errors").is_none(), "{response}");
        assert_eq!(ids(&response["data"]["postsByMonth"]), expected[..count]);
        assert_eq!(
            response["data"]["postsByMonth"]["pageInfo"]["hasNextPage"],
            has_next
        );
    }
    // An unknown valid ID between entries is a boundary, not a lookup.
    for after in [expected[19].clone(), (start + 11).to_string()] {
        let response = query(
            &router,
            MONTH_QUERY,
            json!({"year":2026,"month":11,"after":after}),
        )
        .await;
        assert!(response.get("errors").is_none(), "{response}");
        let connection = &response["data"]["postsByMonth"];
        assert_eq!(ids(connection), expected[20..]);
        assert_eq!(connection["totalCount"], 25);
        assert_eq!(
            connection["pageInfo"],
            json!({"endCursor":expected[24],"hasNextPage":false})
        );
    }
    for after in [start - 1, start] {
        let response = query(
            &router,
            MONTH_QUERY,
            json!({"year":2026,"month":11,"after":after.to_string()}),
        )
        .await;
        assert!(response.get("errors").is_none(), "{response}");
        assert_eq!(
            response["data"]["postsByMonth"],
            json!({"nodes":[],"totalCount":25,"pageInfo":{"endCursor":null,"hasNextPage":false}})
        );
    }
    let empty = query(&router, MONTH_QUERY, json!({"year":2027,"month":1})).await;
    assert!(empty.get("errors").is_none(), "{empty}");
    assert_eq!(
        empty["data"]["postsByMonth"],
        json!({"nodes":[],"totalCount":0,"pageInfo":{"endCursor":null,"hasNextPage":false}})
    );
    let unchanged = data(
        &router,
        "{ posts { totalCount } rust: posts(tag:\"Rust\") { totalCount } }",
    )
    .await;
    assert_eq!(unchanged["posts"]["totalCount"], 27);
    assert_eq!(unchanged["rust"]["totalCount"], 13);

    std::fs::remove_file(root.path().join(format!("contents/{}.md", expected[19]))).unwrap();
    catalog.reconcile().await.unwrap();
    let response = query(
        &router,
        MONTH_QUERY,
        json!({"year":2026,"month":11,"after":expected[19]}),
    )
    .await;
    assert!(response.get("errors").is_none(), "{response}");
    assert_eq!(ids(&response["data"]["postsByMonth"]), expected[20..]);
    assert_eq!(response["data"]["postsByMonth"]["totalCount"], 24);
    assert_eq!(renderer.calls(), 0);
}

#[tokio::test]
async fn monthly_invalid_inputs_and_schema_requirements_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    article(root.path(), 1, "Earliest", &[], "Body");
    let (_, renderer, router) = app(root.path()).await;
    for variables in [
        json!({"year":1969,"month":1}),
        json!({"year":10000,"month":1}),
        json!({"year":-1,"month":1}),
        json!({"year":2026,"month":0}),
        json!({"year":2026,"month":13}),
        json!({"year":2026,"month":256}),
        json!({"year":2026,"month":-1}),
        json!({"year":2026,"month":11,"first":0}),
        json!({"year":2026,"month":11,"first":101}),
        json!({"year":2026,"month":11,"after":"0"}),
        json!({"year":2026,"month":11,"after":"01"}),
        json!({"year":2026,"month":11,"after":"invalid"}),
    ] {
        let response = query(&router, MONTH_QUERY, variables).await;
        assert_eq!(
            response["errors"][0]["extensions"]["code"], "BAD_USER_INPUT",
            "{response}"
        );
        assert_eq!(response["errors"][0]["path"], json!(["postsByMonth"]));
    }
    for invalid in [
        "{ postsByMonth(month:11) { totalCount } }",
        "{ postsByMonth(year:2026) { totalCount } }",
        "{ postsByMonth(year:2026, month:null) { totalCount } }",
        "{ postsByMonth(year:2026, month:11, tag:\"Rust\") { totalCount } }",
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::post("/graphql")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"query":invalid}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let response: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            response["errors"][0]["extensions"]["code"], "GRAPHQL_VALIDATION_FAILED",
            "{response}"
        );
    }
    assert_eq!(renderer.calls(), 0);
}

#[tokio::test]
async fn monthly_posts_keep_real_body_and_resource_resolution() {
    let root = tempfile::tempdir().unwrap();
    let id = millis(datetime!(2026-11-15 12:00 UTC));
    article(
        root.path(),
        id,
        "Monthly body",
        &["Rust"],
        "# Month\n\nA **real** body.",
    );
    let (_, renderer, router) = app(root.path()).await;
    let value = data(&router, "{ postsByMonth(year:2026, month:11) { nodes { id title blocks { kind html } assets { id } } } }").await;
    let post = &value["postsByMonth"]["nodes"][0];
    assert_eq!(post["id"], id.to_string());
    assert_eq!(post["title"], "Monthly body");
    let html: String = post["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|block| block["html"].as_str().unwrap())
        .collect();
    assert!(html.contains("<h1"));
    assert!(html.contains("<strong>real</strong>"));
    assert!(post["assets"].is_array());
    assert_eq!(renderer.calls(), 1);
}
