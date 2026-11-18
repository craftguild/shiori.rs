mod common;
use axum::http::Method;
use common::images::*;

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
    let result = document(&router).await;
    assert_eq!(
        renderer.images.inspections(),
        1,
        "references deduplicated by actual local path"
    );
    let images = image_nodes(&result);
    assert_eq!(images.len(), 8);
    let mut alts = Vec::new();
    for image in images {
        let alternative: String = image["altIds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| {
                result["nodes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|node| node["id"] == *id)
                    .unwrap()["text"]
                    .as_str()
                    .unwrap()
            })
            .collect();
        alts.push(alternative);
        let presentation = &image["presentation"];
        assert_eq!(presentation["sizes"], "100vw");
        assert_eq!(presentation["width"], 700);
        assert_eq!(presentation["height"], 350);
        assert_eq!(
            presentation["url"],
            "https://cms.example/blog/images/1280/a%20%5B%CE%BB%5D,photo.png?mode=original&width=420#caption"
        );
        assert_eq!(
            presentation["candidates"][0]["url"],
            "https://cms.example/blog/images/320/a%20%5B%CE%BB%5D,photo.png?mode=original&width=420#caption"
        );
        assert_eq!(
            presentation["candidates"]
                .as_array()
                .unwrap()
                .iter()
                .map(|candidate| candidate["width"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            [320, 640, 700]
        );
        assert_eq!(image["destination"], url);
    }
    assert_eq!(
        alts,
        [
            "Figure", "Inline", "Linked", "Heading", "Quote", "List", "Table", "Footnote"
        ]
    );
    assert!(
        image_nodes(&result)
            .iter()
            .any(|node| node["title"] == "Caption")
    );
    let count = renderer.calls();
    assert_eq!(document(&router).await, result);
    assert_eq!(renderer.calls(), count);
}

#[tokio::test]
async fn image_only_changes_and_missing_unsupported_transitions_invalidate_document_with_same_article_snapshot()
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
    let first = document(&router).await;
    assert_eq!(candidate_widths(&first), [7]);
    assert!(first.to_string().contains("/assets/missing.png"));
    assert!(first.to_string().contains("/assets/u.svg"));
    let original_calls = renderer.calls();
    assert_eq!(document(&router).await, first);
    assert_eq!(renderer.calls(), original_calls);
    asset(root.path(), "p.png", &png(600, 200, false));
    let changed = document(&router).await;
    assert_eq!(candidate_widths(&changed), [320, 600]);
    assert_eq!(renderer.calls(), original_calls + 1);
    asset(root.path(), "missing.png", &png(8, 12, false));
    asset(root.path(), "u.svg", &png(9, 13, false));
    let added = document(&router).await;
    assert_eq!(candidate_widths(&added), [320, 600, 8, 9]);
    std::fs::remove_file(root.path().join("assets/p.png")).unwrap();
    asset(root.path(), "u.svg", b"<svg/>");
    let deleted = document(&router).await;
    assert_eq!(candidate_widths(&deleted), [8]);
    assert!(deleted.to_string().contains("/assets/p.png"));
    assert!(deleted.to_string().contains("/assets/u.svg"));
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
    let value = document(&router).await;
    let output = value.to_string();
    assert!(output.contains("https://external.example/a.png"));
    assert!(output.contains("assets/a.png"));
    assert!(output.contains("https://cms.example/blog/assets/a.svg"));
    assert!(candidate_widths(&value).is_empty());
    assert_eq!(renderer.images.inspections(), 2);
}

#[tokio::test]
async fn equal_size_and_mtime_replacement_still_changes_document_and_variant_pixels() {
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
    assert_eq!(candidate_widths(&document(&router).await), [21]);
    let original = get(&router, Method::GET, "/images/auto/same.png", &[]).await;
    asset(root.path(), "same.png", &second);
    File::open(&file)
        .unwrap()
        .set_times(FileTimes::new().set_modified(modified))
        .unwrap();
    assert_eq!(file.metadata().unwrap().modified().unwrap(), modified);
    assert_eq!(candidate_widths(&document(&router).await), [31]);
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
    let output = document(&router).await;
    assert_eq!(image_nodes(&output).len(), 4);
    for node in image_nodes(&output) {
        let image = &node["presentation"];
        assert!(image["width"].is_null());
        assert!(image["height"].is_null());
        assert!(image["sizes"].is_null());
        assert!(image["candidates"].as_array().unwrap().is_empty());
        assert!(image["url"].as_str().unwrap().contains("/assets/"));
    }
}
