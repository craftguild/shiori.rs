mod common;
use axum::http::Method;
use common::images::*;
use serde_json::json;

async fn html(router: &axum::Router) -> String {
    let data = common::data(
        router,
        "{ post(id:\"1\") { blocks { id kind html } assets { id kind mimeType content } } }",
    )
    .await;
    data["post"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["html"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn every_parsed_placement_uses_real_typed_presentation_without_eager_inspection() {
    let root = tempfile::tempdir().unwrap();
    let url = "/assets/a%20%5B%CE%BB%5D,photo.png?mode=original&width=420#caption";
    let body = format!(
        "![Figure]({url} \"Caption\")\n\ninline ![Inline]({url}) and [![Linked]({url})](https://example.com)\n\n# ![Heading]({url})\n\n> ![Quote]({url})\n\n- ![List]({url})\n\n| Header |\n| --- |\n| ![Table]({url}) |\n\nref[^one]\n\n[^one]: ![Footnote]({url})\n"
    );
    common::article(root.path(), 1, "All placements", &[], &body);
    asset(root.path(), "a [λ],photo.png", &png(700, 350, false));
    let (_, renderer, router) = common::app(root.path()).await;
    assert_eq!(renderer.images.inspections(), 0);
    common::data(
        &router,
        "{ posts { nodes { id title } } tags { name count } }",
    )
    .await;
    assert_eq!(renderer.images.inspections(), 0);
    let result = html(&router).await;
    assert_eq!(
        renderer.images.inspections(),
        1,
        "references deduplicated by actual local path"
    );
    assert_eq!(result.matches("srcset=").count(), 8, "{result}");
    for alternative in [
        "Figure", "Inline", "Linked", "Heading", "Quote", "List", "Table", "Footnote",
    ] {
        assert!(
            result.contains(&format!("alt=\"{alternative}\"")),
            "{result}"
        );
    }
    assert!(result.contains("Caption"));
    assert!(result.contains("sizes=\"100vw\""));
    assert!(result.contains("width=\"700\" height=\"350\""));
    assert!(result.contains("max-width: min(100%, 420px)"));
    assert!(!result.contains("300px"));
    assert!(result.contains("https://cms.example/blog/images/1280/a%20%5B%CE%BB%5D,photo.png?mode=original&amp;width=420#caption"));
    assert!(result.contains(
        "images/320/a%20%5B%CE%BB%5D%2Cphoto.png?mode=original&amp;width=420#caption 320w"
    ));
    assert!(result.contains("#caption 640w") && result.contains("#caption 700w"));
    assert!(!result.contains(" 960w"));
    let count = renderer.calls();
    assert_eq!(html(&router).await, result);
    assert_eq!(renderer.calls(), count);
}

#[tokio::test]
async fn image_only_changes_and_missing_unsupported_transitions_invalidate_html_with_same_article_snapshot()
 {
    let root = tempfile::tempdir().unwrap();
    common::article(
        root.path(),
        1,
        "Dependency",
        &[],
        "![Small](/assets/p.png) ![Missing](/assets/missing.png) ![Unsupported](/assets/u.svg)\n",
    );
    asset(root.path(), "p.png", &png(7, 11, false));
    asset(root.path(), "u.svg", b"<svg/>");
    let (catalog, renderer, router) = common::app(root.path()).await;
    let snapshot = catalog.snapshot();
    let first = html(&router).await;
    assert_eq!(first.matches("srcset=").count(), 1);
    assert!(first.contains(" 7w"));
    assert!(!first.contains(" 320w"));
    assert!(first.contains("/assets/missing.png"));
    assert!(first.contains("/assets/u.svg"));
    let original_calls = renderer.calls();
    assert_eq!(html(&router).await, first);
    assert_eq!(renderer.calls(), original_calls);
    asset(root.path(), "p.png", &png(600, 200, false));
    let changed = html(&router).await;
    assert!(changed.contains(" 600w"));
    assert!(!changed.contains(" 7w"));
    assert_eq!(renderer.calls(), original_calls + 1);
    asset(root.path(), "missing.png", &png(8, 12, false));
    asset(root.path(), "u.svg", &png(9, 13, false));
    let added = html(&router).await;
    assert_eq!(added.matches("srcset=").count(), 3);
    assert!(added.contains(" 8w"));
    assert!(added.contains(" 9w"));
    std::fs::remove_file(root.path().join("assets/p.png")).unwrap();
    asset(root.path(), "u.svg", b"<svg/>");
    let deleted = html(&router).await;
    assert_eq!(deleted.matches("srcset=").count(), 1);
    assert!(deleted.contains("/assets/p.png"));
    assert!(deleted.contains("/assets/u.svg"));
    assert!(
        std::sync::Arc::ptr_eq(&snapshot, &catalog.snapshot()),
        "asset work cannot publish a new article snapshot"
    );
    assert_eq!(
        get(&router, Method::GET, "/images/auto/missing.png", &[])
            .await
            .0,
        200
    );
}

#[tokio::test]
async fn external_relative_unsupported_and_bad_images_keep_original_delivery_and_metadata_isolated()
{
    let root = tempfile::tempdir().unwrap();
    common::article(
        root.path(),
        1,
        "Fallbacks",
        &[],
        "![External](https://external.example/a.png) ![Relative](assets/a.png) ![Svg](/assets/a.svg) ![Bad](/assets/bad.png)\n",
    );
    asset(root.path(), "a.svg", b"<svg/>");
    asset(root.path(), "bad.png", b"broken");
    let (_, renderer, router) = common::app(root.path()).await;
    let value = common::query(
        &router,
        "{ post(id:\"1\") { title blocks { id kind html } assets { id } } }",
        json!({}),
    )
    .await;
    assert!(value.get("errors").is_none(), "{value}");
    let output = value.to_string();
    assert!(output.contains("https://external.example/a.png"));
    assert!(output.contains("assets/a.png"));
    assert!(output.contains("https://cms.example/blog/assets/a.svg"));
    assert!(!output.contains("srcset"));
    assert_eq!(renderer.images.inspections(), 2);
}

#[tokio::test]
async fn equal_size_and_mtime_replacement_still_changes_html_and_variant_pixels() {
    use img_parts::png::{Png, PngChunk};
    use std::fs::{File, FileTimes};
    let root = tempfile::tempdir().unwrap();
    common::article(
        root.path(),
        1,
        "Hash dependency",
        &[],
        "![A](/assets/same.png)",
    );
    let first = png(21, 31, false);
    let second = png(31, 21, false);
    let target = first.len().max(second.len()) + 1024;
    let padded = |bytes: Vec<u8>| {
        let count = target - bytes.len() - 12;
        let mut container = Png::from_bytes(bytes.into()).unwrap();
        let last = container.chunks().len() - 1;
        let mut contents = b"padding\0".to_vec();
        contents.resize(count, b'a');
        container
            .chunks_mut()
            .insert(last, PngChunk::new(*b"tEXt", contents.into()));
        container.encoder().bytes().to_vec()
    };
    let first = padded(first);
    let second = padded(second);
    assert_eq!(first.len(), second.len());
    asset(root.path(), "same.png", &first);
    let file = root.path().join("assets/same.png");
    let modified = file.metadata().unwrap().modified().unwrap();
    let (_, renderer, router) = common::app(root.path()).await;
    assert!(html(&router).await.contains(" 21w"));
    let original = get(&router, Method::GET, "/images/auto/same.png", &[]).await;
    asset(root.path(), "same.png", &second);
    File::open(&file)
        .unwrap()
        .set_times(FileTimes::new().set_modified(modified))
        .unwrap();
    assert_eq!(file.metadata().unwrap().modified().unwrap(), modified);
    assert!(html(&router).await.contains(" 31w"));
    assert_eq!(renderer.calls(), 2);
    let changed = get(&router, Method::GET, "/images/auto/same.png", &[]).await;
    assert_ne!(original.1["etag"], changed.1["etag"]);
    assert_eq!(decoded(&changed.2).dimensions(), (31, 21));
}

#[tokio::test]
async fn original_fallback_uses_native_layout_without_speculative_pixel_attributes() {
    let root = tempfile::tempdir().unwrap();
    common::article(
        root.path(),
        1,
        "Original layout",
        &[],
        "![Svg](/assets/a.svg) ![Gif](/assets/a.gif) ![Corrupt](/assets/bad.png) ![Missing](/assets/missing.png)",
    );
    asset(
        root.path(),
        "a.svg",
        b"<svg xmlns='http://www.w3.org/2000/svg' width='8' height='12'/>",
    );
    asset(root.path(), "a.gif", GIF);
    asset(root.path(), "bad.png", b"corrupt");
    let (_, _, router) = common::app(root.path()).await;
    let output = html(&router).await;
    assert_eq!(output.matches("<img ").count(), 4);
    assert_eq!(
        output
            .matches("width: auto; height: auto; max-width: 100%;")
            .count(),
        4
    );
    assert!(!output.contains("300px"));
    assert!(!output.contains("srcset="));
    assert!(!output.contains("width=\""));
    assert!(!output.contains("height=\""));
}
