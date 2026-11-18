#![allow(dead_code)]
pub const GIF: &[u8] = b"GIF89a\x01\0\x01\0\x80\0\0\0\0\0\xff\xff\xff!\xf9\x04\x01\0\0\0\0,\0\0\0\0\x01\0\x01\0\0\x02\x02D\x01\0;";
use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use std::{io::Cursor, path::Path};
use tower::ServiceExt;

pub async fn document(router: &Router) -> serde_json::Value {
    super::data(router, "{post(id:\"1\"){document{nodes{__typename ... on TextNode{id text} ... on ImageNode{id altIds destination title presentation{url originalUrl candidates{url width} sizes width height}}}}}}")
        .await["post"]["document"].clone()
}
pub fn image_nodes(document: &serde_json::Value) -> Vec<&serde_json::Value> {
    document["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|node| node["__typename"] == "ImageNode")
        .collect()
}
pub fn alternative(document: &serde_json::Value, image: &serde_json::Value) -> String {
    image["altIds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| {
            document["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|node| node["id"] == *id)
                .unwrap()["text"]
                .as_str()
                .unwrap()
        })
        .collect()
}
pub fn candidate_widths(document: &serde_json::Value) -> Vec<u64> {
    image_nodes(document)
        .iter()
        .flat_map(|node| node["presentation"]["candidates"].as_array().unwrap())
        .map(|candidate| candidate["width"].as_u64().unwrap())
        .collect()
}

pub fn pixels(width: u32, height: u32, transparent: bool) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, y| {
        if transparent {
            if x < width / 2 {
                Rgba([250, 0, 0, 255])
            } else {
                Rgba([0, 0, 250, 0])
            }
        } else {
            Rgba([(x % 251) as u8, (y % 251) as u8, ((x + y) % 251) as u8, 255])
        }
    })
}
pub fn png(width: u32, height: u32, transparent: bool) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(pixels(width, height, transparent))
        .write_to(&mut out, ImageFormat::Png)
        .unwrap();
    out.into_inner()
}
pub fn jpeg(width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 100)
        .encode_image(&DynamicImage::ImageRgba8(pixels(width, height, false)).into_rgb8())
        .unwrap();
    out
}
pub fn asset(root: &Path, name: &str, bytes: &[u8]) {
    let path = root.join("assets").join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}
pub async fn get(
    router: &Router,
    method: Method,
    path: &str,
    headers: &[(&str, &str)],
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut request = Request::builder().method(method).uri(path);
    for (key, value) in headers {
        request = request.header(*key, *value);
    }
    let response = router
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    (status, headers, bytes)
}
pub fn decoded(bytes: &[u8]) -> RgbaImage {
    image::load_from_memory(bytes).unwrap().into_rgba8()
}
