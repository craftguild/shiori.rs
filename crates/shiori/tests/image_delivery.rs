mod common;
use axum::http::{Method, StatusCode};
use common::images::*;
use image::{DynamicImage, ImageFormat};
use img_parts::{ImageEXIF, ImageICC};
use std::{io::Cursor, os::unix::fs::symlink};

#[tokio::test]
async fn every_width_negotiates_real_pixels_with_no_upscale_and_original_bytes_stay_exact() {
    let root = tempfile::tempdir().unwrap();
    common::article(root.path(), 1, "Images", &[], "");
    let original = jpeg(2700, 1800);
    asset(root.path(), "photo.jpg", &original);
    let (_, renderer, router) = common::app(root.path()).await;
    assert_eq!(renderer.images.inspections(), 0);
    for (token, width) in [
        ("320", 320),
        ("640", 640),
        ("960", 960),
        ("1280", 1280),
        ("1920", 1920),
        ("2560", 2560),
        ("auto", 2560),
    ] {
        for (accept, mime) in [
            ("image/webp,image/jpeg;q=0.8", "image/webp"),
            ("image/jpeg", "image/jpeg"),
        ] {
            let (status, headers, bytes) = get(
                &router,
                Method::GET,
                &format!("/images/{token}/photo.jpg"),
                &[("accept", accept), ("origin", "https://frontend.example")],
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{token} {accept}");
            assert_eq!(headers["content-type"], mime);
            assert_eq!(headers["access-control-allow-origin"], "*");
            let pixels = decoded(&bytes);
            assert_eq!(
                pixels.dimensions(),
                (width, ((1800u64 * u64::from(width) + 1350) / 2700) as u32)
            );
            if token == "320" {
                assert!(bytes.len() < original.len() / 2);
            }
        }
    }
    let (status, _, bytes) = get(
        &router,
        Method::GET,
        "/assets/photo.jpg",
        &[("accept", "image/webp")],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, original);
    let tiny = png(7, 11, false);
    asset(root.path(), "tiny", &tiny);
    for token in ["320", "640", "960", "1280", "1920", "2560", "auto"] {
        let (status, headers, bytes) =
            get(&router, Method::GET, &format!("/images/{token}/tiny"), &[]).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers["content-type"], "image/png");
        assert_eq!(decoded(&bytes).dimensions(), (7, 11));
        assert_eq!(bytes, tiny);
    }
}

#[tokio::test]
async fn png_and_webp_keep_alpha_and_lanczos_does_not_mix_hidden_rgb() {
    let root = tempfile::tempdir().unwrap();
    common::article(root.path(), 1, "Alpha", &[], "");
    asset(root.path(), "alpha.jpg", &png(700, 1400, true)); // Contents choose the codec.
    let (_, _, router) = common::app(root.path()).await;
    for accept in ["image/webp", "image/png"] {
        let (status, _, bytes) = get(
            &router,
            Method::GET,
            "/images/320/alpha.jpg",
            &[("accept", accept)],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let pixels = decoded(&bytes);
        assert_eq!(pixels.dimensions(), (320, 640));
        assert_eq!(pixels.get_pixel(20, 20).0, [250, 0, 0, 255]);
        assert_eq!(pixels.get_pixel(300, 20).0[3], 0);
        for pixel in pixels.pixels().filter(|p| p.0[3] > 5 && p.0[3] < 250) {
            assert!(pixel.0[0] > 240 && pixel.0[2] < 5, "{pixel:?}");
        }
    }
    let encoded = webp::Encoder::from_rgba(pixels(700, 1400, true).as_raw(), 700, 1400)
        .encode_lossless()
        .to_vec();
    asset(root.path(), "alpha.webp", &encoded);
    let (_, headers, bytes) = get(&router, Method::GET, "/images/320/alpha.webp", &[]).await;
    assert_eq!(headers["content-type"], "image/png");
    assert_eq!(decoded(&bytes).get_pixel(300, 20).0[3], 0);
}

#[tokio::test]
async fn accept_specificity_quality_exclusions_head_etag_and_live_change() {
    let root = tempfile::tempdir().unwrap();
    common::article(root.path(), 1, "HTTP", &[], "");
    asset(root.path(), "p", &png(700, 400, false));
    let (_, renderer, router) = common::app(root.path()).await;
    for (accept, mime) in [
        (None, Some("image/png")),
        (Some("*/*"), Some("image/png")),
        (Some("image/*"), Some("image/png")),
        (Some("image/webp,image/png"), Some("image/webp")),
        (Some("image/webp;q=0,image/*;q=1"), Some("image/png")),
        (Some("image/webp;q=0.7,image/png;q=0.9"), Some("image/png")),
        (Some("image/webp;q=0.9,image/png;q=0.1"), Some("image/webp")),
        (Some("image/png;q=0,image/webp;q=0,*/*;q=1"), None),
        (Some("text/html"), None),
        (Some(""), None),
        (Some("image/webp;q=2"), None),
    ] {
        let extra = accept.map(|v| vec![("accept", v)]).unwrap_or_default();
        let (status, headers, body) = get(
            &router,
            Method::GET,
            "/images/320/p?ignored=quality#fragment",
            &extra,
        )
        .await;
        if let Some(mime) = mime {
            assert_eq!(status, StatusCode::OK, "{accept:?}");
            assert_eq!(headers["content-type"], mime);
            assert!(!body.is_empty());
        } else {
            assert_eq!(status, StatusCode::NOT_ACCEPTABLE, "{accept:?}");
        }
    }
    let (_, get_headers, bytes) = get(
        &router,
        Method::GET,
        "/images/320/p",
        &[("accept", "image/webp")],
    )
    .await;
    for (key, value) in [
        ("cache-control", "no-cache"),
        ("x-content-type-options", "nosniff"),
    ] {
        assert_eq!(get_headers[key], value);
    }
    assert!(
        get_headers
            .get_all("vary")
            .iter()
            .any(|v| v.to_str().unwrap().contains("Accept"))
    );
    let count = renderer.images.transforms();
    let (status, head, empty) = get(
        &router,
        Method::HEAD,
        "/images/320/p",
        &[("accept", "image/webp")],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(empty.is_empty());
    for key in ["etag", "content-type", "content-length", "cache-control"] {
        assert_eq!(head[key], get_headers[key]);
    }
    assert_eq!(renderer.images.transforms(), count);
    let etag = get_headers["etag"].to_str().unwrap();
    let (status, headers, body) = get(
        &router,
        Method::GET,
        "/images/320/p",
        &[("accept", "image/webp"), ("if-none-match", etag)],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_MODIFIED);
    assert!(body.is_empty());
    assert_eq!(headers["etag"], etag);
    let (_, headers, conventional) = get(
        &router,
        Method::GET,
        "/images/320/p",
        &[("accept", "image/png"), ("if-none-match", etag)],
    )
    .await;
    assert_ne!(headers["etag"], etag);
    assert_ne!(bytes, conventional);
    asset(root.path(), "p", &png(900, 450, false));
    let (_, headers, bytes) = get(
        &router,
        Method::GET,
        "/images/320/p",
        &[("accept", "image/webp")],
    )
    .await;
    assert_ne!(headers["etag"], etag);
    assert_eq!(decoded(&bytes).dimensions(), (320, 160));
}

#[tokio::test]
async fn route_errors_follow_capability_boundaries_and_preserve_unsupported_originals() {
    let root = tempfile::tempdir().unwrap();
    common::article(root.path(), 1, "Safety", &[], "");
    asset(root.path(), "broken", b"\x89PNG\r\n\x1a\ntruncated");
    asset(
        root.path(),
        "vector.svg",
        b"<svg xmlns='http://www.w3.org/2000/svg'/>",
    );
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageLuma16(image::ImageBuffer::from_pixel(4, 4, image::Luma([1234u16])))
        .write_to(&mut out, ImageFormat::Png)
        .unwrap();
    asset(root.path(), "deep.png", &out.into_inner());
    symlink(
        root.path().join("articles/1.md"),
        root.path().join("assets/link"),
    )
    .unwrap();
    std::fs::create_dir(root.path().join("assets/dir")).unwrap();
    let (_, _, router) = common::app(root.path()).await;
    for (path, status) in [
        ("/images/321/vector.svg", 400),
        ("/images/0320/vector.svg", 400),
        ("/images/auto/%2e%2e/articles/1.md", 400),
        ("/images/auto/%5csecret", 400),
        ("/images/auto/%00", 400),
        ("/images/auto/nope", 404),
        ("/images/auto/link", 404),
        ("/images/auto/dir", 404),
        ("/images/auto/vector.svg", 415),
        ("/images/auto/deep.png", 415),
        ("/images/auto/broken", 422),
    ] {
        let (actual, _, body) = get(&router, Method::GET, path, &[]).await;
        assert_eq!(actual.as_u16(), status, "{path}");
        assert!(body.is_empty());
    }
    let original = std::fs::read(root.path().join("assets/vector.svg")).unwrap();
    assert_eq!(
        get(&router, Method::GET, "/assets/vector.svg", &[]).await.2,
        original
    );
}

#[tokio::test]
async fn orientation_and_rgb_icc_survive_resize_while_exif_is_removed() {
    let root = tempfile::tempdir().unwrap();
    common::article(root.path(), 1, "Metadata", &[], "");
    let mut container = img_parts::jpeg::Jpeg::from_bytes(jpeg(800, 400).into()).unwrap();
    // TIFF little-endian, one Orientation SHORT entry with value6 (90deg CW).
    let exif = vec![
        b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 1, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0,
    ];
    container.set_exif(Some(exif.into()));
    let xmp = b"http://ns.adobe.com/xap/1.0/\0<x:xmpmeta>private authoring marker</x:xmpmeta>";
    container.segments_mut().insert(
        1,
        img_parts::jpeg::JpegSegment::new_with_contents(
            img_parts::jpeg::markers::APP1,
            xmp.to_vec().into(),
        ),
    );
    let original = container.encoder().bytes().to_vec();
    asset(root.path(), "oriented", &original);
    let (_, _, router) = common::app(root.path()).await;
    let (status, _, bytes) = get(
        &router,
        Method::GET,
        "/images/320/oriented",
        &[("accept", "image/jpeg")],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let decoded = decoded(&bytes);
    assert_eq!(decoded.dimensions(), (320, 640));
    let corner = decoded.get_pixel(32, 64).0;
    // 90deg CW maps this output point into the original gradient near (80,359).
    for (actual, expected) in corner[..3].iter().zip([80u8, 108, 188]) {
        assert!(
            actual.abs_diff(expected) <= 12,
            "orientation pixels: {corner:?}"
        );
    }
    let output = img_parts::jpeg::Jpeg::from_bytes(bytes.into()).unwrap();
    assert!(output.exif().is_none());
    assert!(
        output
            .segments()
            .iter()
            .all(|s| !s.contents().starts_with(b"http://ns.adobe.com/xap/1.0/\0"))
    );
    assert!(output.icc_profile().is_none());
    // An unsupported CMYK profile is never attached to converted RGB pixels.
    let mut profile = vec![0u8; 132];
    profile[..4].copy_from_slice(&132u32.to_be_bytes());
    profile[16..20].copy_from_slice(b"CMYK");
    profile[36..40].copy_from_slice(b"acsp");
    let mut container = img_parts::jpeg::Jpeg::from_bytes(jpeg(800, 400).into()).unwrap();
    container.set_icc_profile(Some(profile.into()));
    let original = container.encoder().bytes().to_vec();
    asset(root.path(), "unsupported", &original);
    assert_eq!(
        get(&router, Method::GET, "/images/320/unsupported", &[])
            .await
            .0,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    assert_eq!(
        get(&router, Method::GET, "/assets/unsupported", &[])
            .await
            .2,
        original
    );
}
