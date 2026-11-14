use super::{AssetPath, Error, ImageService, Width};
use axum::{
    body::Body,
    extract::{OriginalUri, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use std::sync::Arc;

pub(crate) async fn image(
    State(images): State<Arc<ImageService>>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    method: Method,
) -> Response {
    let result = async {
        let tail = uri.path().strip_prefix("/images/").ok_or(Error::Path)?;
        let (width, path) = tail.split_once('/').ok_or(Error::Path)?;
        let width = Width::parse(width)?;
        let path = AssetPath::parse(path)?;
        let mut accept = String::new();
        for value in headers.get_all(header::ACCEPT) {
            if !accept.is_empty() {
                accept.push(',');
            }
            accept.push_str(value.to_str().map_err(|_| Error::NotAcceptable)?);
        }
        images
            .variant(
                path,
                width,
                headers.contains_key(header::ACCEPT).then_some(accept),
            )
            .await
    }
    .await;
    let mut response = match result {
        Ok(variant) => {
            let unchanged = headers
                .get_all(header::IF_NONE_MATCH)
                .iter()
                .filter_map(|v| v.to_str().ok())
                .any(|v| {
                    v.split(',').any(|candidate| {
                        candidate.trim() == "*"
                            || candidate.trim().trim_start_matches("W/") == variant.etag
                    })
                });
            let mut response = if unchanged {
                StatusCode::NOT_MODIFIED.into_response()
            } else {
                Response::new(if method == Method::HEAD {
                    Body::empty()
                } else {
                    Body::from(variant.bytes.clone())
                })
            };
            let values = response.headers_mut();
            values.insert(
                header::ETAG,
                HeaderValue::from_str(&variant.etag).expect("digest"),
            );
            values.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static(variant.format.mime()),
            );
            if !unchanged {
                values.insert(
                    header::CONTENT_LENGTH,
                    HeaderValue::from(variant.bytes.len()),
                );
            }
            response
        }
        Err(error) => {
            tracing::debug!(event="image_unavailable",path=%uri.path(),%error);
            match error {
                Error::Path => StatusCode::BAD_REQUEST,
                Error::Missing => StatusCode::NOT_FOUND,
                Error::Unsupported => StatusCode::UNSUPPORTED_MEDIA_TYPE,
                Error::Decode | Error::OutputLimit | Error::Unstable => {
                    StatusCode::UNPROCESSABLE_ENTITY
                }
                Error::Busy => StatusCode::SERVICE_UNAVAILABLE,
                Error::Internal => StatusCode::INTERNAL_SERVER_ERROR,
                Error::NotAcceptable => StatusCode::NOT_ACCEPTABLE,
            }
            .into_response()
        }
    };
    let headers = response.headers_mut();
    headers.insert(header::VARY, HeaderValue::from_static("Accept"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}
