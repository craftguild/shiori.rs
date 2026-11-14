//! Repeatable real-image inputs and independent decoding of release HTTP output.
use super::images::*;
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use img_parts::ImageEXIF;
use serde_json::{Value, json};
use std::{io::Cursor, path::Path};

fn solid(width: u32, height: u32, alpha: bool) -> Vec<u8> {
    let image = RgbaImage::from_fn(width, height, |x, _| {
        if alpha && x >= width / 2 {
            Rgba([0, 0, 250, 0])
        } else {
            Rgba([240, 20, 40, 255])
        }
    });
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

pub(crate) fn export(root: &Path) {
    assert!(
        root.read_dir().unwrap().next().is_none(),
        "fixture export needs an empty temporary directory"
    );
    asset(root, "tiny.gif", GIF);
    asset(root, "height-bound.png", &solid(640, 320, false));
    let mut wide = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut wide, 100)
        .encode_image(&DynamicImage::ImageRgba8(pixels(2700, 1350, false)).into_rgb8())
        .unwrap();
    asset(root, "wide [λ],photo.jpg", &wide);
    asset(root, "alpha.png", &solid(1400, 700, true));
    asset(root, "tiny.png", &solid(7, 11, false));
    asset(root, "portrait.png", &solid(300, 900, false));
    let mut oriented = img_parts::jpeg::Jpeg::from_bytes(jpeg(640, 320).into()).unwrap();
    oriented.set_exif(Some(
        vec![
            b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 1, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0,
        ]
        .into(),
    ));
    asset(root, "oriented.jpg", oriented.encoder().bytes().as_ref());
    asset(root,"vector.svg",b"<svg xmlns='http://www.w3.org/2000/svg' width='8' height='12'><rect width='8' height='12' fill='red'/></svg>");
    let wide = "/assets/wide%20%5B%CE%BB%5D,photo.jpg?authored=yes&width=420#photo";
    let body = format!(
        "![Wide]({wide} \"Wide caption\")\n\ninline ![Inline]({wide}) and [![Linked]({wide})](https://example.com)\n\n# ![Heading]({wide})\n\n> ![Quote]({wide})\n\n- ![List]({wide})\n\n| Header |\n| --- |\n| ![Table]({wide}) |\n\nref[^one]\n\n[^one]: ![Footnote]({wide})\n\n![Alpha](/assets/alpha.png \"Alpha caption\")\n\n![Tiny](/assets/tiny.png)\n\n![Portrait](/assets/portrait.png)\n\n![Oriented](/assets/oriented.jpg)\n\n![Vector](/assets/vector.svg)\n\n![Gif](/assets/tiny.gif)\n\n![Height bound](/assets/height-bound.png?height=20 \"Bounded caption\")\n"
    );
    super::article(root, 1, "Responsive release fixture", &[], &body);
    std::fs::write(root.join("fixture.json"),serde_json::to_vec_pretty(&json!({"wide":{"width":2700,"height":1350},"alpha":{"width":1400,"height":700},"tiny":{"width":7,"height":11},"portrait":{"width":300,"height":900},"oriented":{"width":320,"height":640}})).unwrap()).unwrap();
}

pub(crate) fn verify_download(bytes: &[u8], row: &Value) -> image::RgbaImage {
    let image = decoded(bytes);
    assert_eq!(
        image.width(),
        row["width"].as_u64().unwrap() as u32,
        "{row}"
    );
    assert_eq!(
        image.height(),
        row["height"].as_u64().unwrap() as u32,
        "{row}"
    );
    let kind = row["kind"].as_str().unwrap();
    if ["alpha", "tiny", "portrait", "bounded"].contains(&kind) {
        let pixel = image.get_pixel(image.width() / 4, image.height() / 2).0;
        assert!(
            pixel[0].abs_diff(240) <= 3
                && pixel[1].abs_diff(20) <= 3
                && pixel[2].abs_diff(40) <= 3
                && pixel[3] == 255,
            "{row}: {pixel:?}"
        );
        if kind == "alpha" {
            assert_eq!(
                image.get_pixel(image.width() * 3 / 4, image.height() / 2).0[3],
                0
            );
        }
    }
    if kind == "wide" || kind == "oriented" {
        let pixel = image.get_pixel(image.width() / 2, image.height() / 2).0;
        let expected = if kind == "wide" {
            [95u8, 173, 17]
        } else {
            [69u8, 159, 228]
        };
        for channel in 0..3 {
            assert!(
                pixel[channel].abs_diff(expected[channel]) <= 22,
                "{row}: {pixel:?}"
            );
        }
        assert_eq!(pixel[3], 255);
    }
    image
}
