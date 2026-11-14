//! Bind the official GraphiQL view to local routes behind an optional public prefix.
use anyhow::{Context, Result};
use axum::{
    Router,
    body::Body,
    extract::OriginalUri,
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
    routing::any,
};
use chigiri::graphiql::{GraphiqlView, Paths};
use url::Url;

pub(crate) fn paths(public_base: &Url) -> Result<Paths> {
    let prefix = public_base.path().trim_end_matches('/');
    if prefix.is_empty() {
        Ok(Paths::default())
    } else {
        Paths::default().with_prefix(prefix).context(
            "GraphiQL public URL prefix must use static ASCII path segments (letters, digits, /._~-)",
        )
    }
}

pub(crate) fn routes<S: Clone + Send + Sync + 'static>(
    view: GraphiqlView,
    paths: Paths,
) -> Router<S> {
    let prefix = paths
        .ui()
        .strip_suffix("/graphiql")
        .expect("fixed UI suffix")
        .to_owned();
    let handler = move |OriginalUri(uri): OriginalUri, method: Method| {
        // The proxy strips the public prefix, like the existing /graphql route.
        // Only validated static paths enter the HTML; Host and query are unused.
        let public_path = format!("{prefix}{}", uri.path());
        let parts = view.respond(&paths, method.as_str(), &public_path);
        async move {
            let mut response = Response::builder().status(parts.status);
            for header in parts.headers {
                response = response.header(header.name, header.value);
            }
            response
                .body(Body::from(parts.body))
                .unwrap_or_else(|error| {
                    tracing::error!(event="graphiql_response_failed", %error);
                    StatusCode::INTERNAL_SERVER_ERROR.into_response()
                })
        }
    };
    Router::new()
        .route("/graphiql", any(handler.clone()))
        .route("/graphiql/assets/{*asset}", any(handler))
}
