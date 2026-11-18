mod common;
use axum::http::{Method, StatusCode};
use common::images::*;
use img_parts::{DynImage, ImageEXIF, ImageICC};

#[tokio::test]
async fn real_rgb_profiles_are_preserved_in_all_containers_and_unknown_profiles_fall_back() {
    let root = tempfile::tempdir().unwrap();
    common::article(root.path(), 1, "ICC", &[], "");
    let rgb = include_bytes!("fixtures/sRGB-v4.icc").to_vec();
    let gray = include_bytes!("fixtures/sGrey-v4.icc").to_vec();
    let inputs = [
        ("png", png(640, 320, false)),
        ("jpeg", jpeg(640, 320)),
        (
            "webp",
            webp::Encoder::from_rgba(pixels(640, 320, false).as_raw(), 640, 320)
                .encode(80.0)
                .to_vec(),
        ),
    ];
    let (_, _, router) = common::app(root.path()).await;
    let cmyk = include_bytes!("fixtures/cmyk.jpg");
    asset(root.path(), "cmyk.jpg", cmyk);
    assert_eq!(
        get(&router, Method::GET, "/images/auto/cmyk.jpg", &[])
            .await
            .0,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    assert_eq!(
        get(&router, Method::GET, "/assets/cmyk.jpg", &[]).await.2,
        cmyk
    );
    for (name, bytes) in inputs {
        let mut container = DynImage::from_bytes(bytes.clone().into()).unwrap().unwrap();
        container.set_icc_profile(Some(rgb.clone().into()));
        let original = container.encoder().bytes().to_vec();
        asset(root.path(), name, &original);
        for accept in ["image/webp", "image/png,image/jpeg"] {
            let (status, _, output) = get(
                &router,
                Method::GET,
                &format!("/images/320/{name}"),
                &[("accept", accept)],
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{name} {accept}");
            assert_eq!(decoded(&output).dimensions(), (320, 160));
            let output = DynImage::from_bytes(output.into()).unwrap().unwrap();
            assert_eq!(output.icc_profile().unwrap().as_ref(), rgb);
            assert!(output.exif().is_none());
        }
        assert_eq!(
            get(&router, Method::GET, &format!("/assets/{name}"), &[])
                .await
                .2,
            original
        );
        let mut container = DynImage::from_bytes(bytes.into()).unwrap().unwrap();
        container.set_icc_profile(Some(gray.clone().into()));
        let original = container.encoder().bytes().to_vec();
        asset(root.path(), "unsupported", &original);
        assert_eq!(
            get(&router, Method::GET, "/images/auto/unsupported", &[])
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
}

#[tokio::test]
async fn real_animations_are_never_flattened_and_originals_are_exact() {
    let root = tempfile::tempdir().unwrap();
    common::article(
        root.path(),
        1,
        "Animations",
        &[],
        "![A](/assets/anim.png) ![B](/assets/anim.webp)",
    );
    let mut apng = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut apng, 2, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_animated(2, 0).unwrap();
        encoder.set_frame_delay(1, 10).unwrap();
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[255; 16]).unwrap();
        writer.write_image_data(&[0; 16]).unwrap();
        writer.finish().unwrap();
    }
    let config = webp::WebPConfig::new().unwrap();
    let first = [255; 16];
    let second = [0; 16];
    let mut encoder = webp::AnimEncoder::new(2, 2, &config);
    encoder.add_frame(webp::AnimFrame::from_rgba(&first, 2, 2, 0));
    encoder.add_frame(webp::AnimFrame::from_rgba(&second, 2, 2, 100));
    let webp = encoder.try_encode().unwrap().to_vec();
    for (name, bytes) in [("anim.png", apng), ("anim.webp", webp)] {
        asset(root.path(), name, &bytes);
    }
    let (_, _, router) = common::app(root.path()).await;
    let gif = GIF;
    asset(root.path(), "tiny.gif", gif);
    assert_eq!(
        get(&router, Method::GET, "/images/auto/tiny.gif", &[])
            .await
            .0,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    assert_eq!(
        get(&router, Method::GET, "/assets/tiny.gif", &[]).await.2,
        gif
    );
    for name in ["anim.png", "anim.webp"] {
        assert_eq!(
            get(&router, Method::GET, &format!("/images/auto/{name}"), &[])
                .await
                .0,
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );
        assert_eq!(
            get(&router, Method::GET, &format!("/assets/{name}"), &[])
                .await
                .2,
            std::fs::read(root.path().join("assets").join(name)).unwrap()
        );
    }
    let result = document(&router).await;
    assert!(candidate_widths(&result).is_empty());
    assert!(result.to_string().contains("/assets/anim.png"));
}

#[tokio::test]
async fn input_pixel_and_dimension_bounds_fail_before_unbounded_work_and_fifos_never_block() {
    use img_parts::png::{Png, PngChunk};
    use std::{fs::File, os::unix::fs::symlink};
    let root = tempfile::tempdir().unwrap();
    common::article(root.path(), 1, "Limits", &[], "");
    asset(root.path(), "small", &png(2, 2, false));
    for (name, w, h) in [("side", 16385u32, 1u32), ("pixels", 9000, 9000)] {
        let mut container = Png::from_bytes(png(2, 2, false).into()).unwrap();
        let ihdr = container
            .chunks_mut()
            .iter_mut()
            .find(|c| c.kind() == *b"IHDR")
            .unwrap();
        let mut data = ihdr.contents().to_vec();
        data[..4].copy_from_slice(&w.to_be_bytes());
        data[4..8].copy_from_slice(&h.to_be_bytes());
        *ihdr = PngChunk::new(*b"IHDR", data.into());
        asset(root.path(), name, container.encoder().bytes().as_ref());
    }
    let oversized = File::create(root.path().join("assets/huge")).unwrap();
    oversized.set_len(32 * 1024 * 1024 + 1).unwrap();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(root.path().join("assets/fifo"))
            .status()
            .unwrap()
            .success()
    );
    symlink(
        root.path().join("assets"),
        root.path().join("assets/nested"),
    )
    .unwrap();
    let (_, _, router) = common::app(root.path()).await;
    for (path, code) in [
        ("side", 422),
        ("pixels", 422),
        ("huge", 404),
        ("fifo", 404),
        ("nested/small", 404),
    ] {
        assert_eq!(
            get(&router, Method::GET, &format!("/images/auto/{path}"), &[])
                .await
                .0
                .as_u16(),
            code,
            "{path}"
        );
    }
}

#[tokio::test]
async fn exif_only_non_srgb_color_is_preserved_as_original_unless_rgb_icc_is_present() {
    let root = tempfile::tempdir().unwrap();
    common::article(root.path(), 1, "EXIF color", &[], "");
    let (_, _, router) = common::app(root.path()).await;
    for color in [1u16, 2, 65535] {
        let mut exif = vec![
            b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x69, 0x87, 4, 0, 1, 0, 0, 0, 26, 0, 0, 0, 0, 0,
            0, 0, 1, 0, 1, 0xa0, 3, 0, 1, 0, 0, 0,
        ];
        exif.extend(color.to_le_bytes());
        exif.extend([0u8; 6]);
        for icc in [false, true] {
            let mut container = img_parts::jpeg::Jpeg::from_bytes(jpeg(640, 320).into()).unwrap();
            container.set_exif(Some(exif.clone().into()));
            if icc {
                container
                    .set_icc_profile(Some(include_bytes!("fixtures/sRGB-v4.icc").to_vec().into()));
            }
            let original = container.encoder().bytes().to_vec();
            asset(root.path(), "exif.jpg", &original);
            let (status, _, bytes) = get(
                &router,
                Method::GET,
                "/images/320/exif.jpg",
                &[("accept", "image/jpeg")],
            )
            .await;
            if color == 1 || icc {
                assert_eq!(status, StatusCode::OK);
                assert_eq!(decoded(&bytes).dimensions(), (320, 160));
            } else {
                assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
            }
            assert_eq!(
                get(&router, Method::GET, "/assets/exif.jpg", &[]).await.2,
                original
            );
        }
    }
}

#[tokio::test]
async fn maximum_valid_portrait_uses_conventional_format_when_webp_geometry_is_unavailable() {
    let root = tempfile::tempdir().unwrap();
    common::article(
        root.path(),
        1,
        "Maximum portrait",
        &[],
        "![Tall](/assets/tall.png)",
    );
    asset(root.path(), "tall.png", &png(1, 16_384, false));
    let (_, _, router) = common::app(root.path()).await;
    for token in ["320", "640", "960", "1280", "1920", "2560", "auto"] {
        let (status, headers, bytes) = get(
            &router,
            Method::GET,
            &format!("/images/{token}/tall.png"),
            &[("accept", "image/webp,image/png;q=0.5")],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers["content-type"], "image/png");
        assert_eq!(decoded(&bytes).dimensions(), (1, 16_384));
    }
    assert_eq!(
        get(
            &router,
            Method::GET,
            "/images/auto/tall.png",
            &[("accept", "image/webp")]
        )
        .await
        .0,
        StatusCode::NOT_ACCEPTABLE
    );
    assert_eq!(candidate_widths(&document(&router).await), [1]);
}
