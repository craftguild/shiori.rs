use std::sync::Arc;

use anyhow::Result;
use axum::{
    Router,
    body::Body,
    extract::{OriginalUri, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use percent_encoding::percent_decode_str;
use tokio::sync::Semaphore;
use tower_http::cors::{Any, CorsLayer};

use crate::{catalog::Catalog, config::Config, graphql, rendering::Renderer, storage};

#[derive(Clone)]
struct Assets {
    root: Arc<crate::asset_root::AssetRoot>,
    workers: Arc<Semaphore>,
}

pub fn router(catalog: Arc<Catalog>, renderer: Arc<Renderer>) -> Result<Router> {
    routes(catalog, renderer, None)
}

pub fn router_with_config(
    catalog: Arc<Catalog>,
    renderer: Arc<Renderer>,
    config: &Config,
) -> Result<Router> {
    let graphiql = config
        .graphiql
        .then(|| crate::graphiql::paths(&config.public_base))
        .transpose()?;
    routes(catalog, renderer, graphiql)
}

fn routes(
    catalog: Arc<Catalog>,
    renderer: Arc<Renderer>,
    graphiql: Option<chigiri::graphiql::Paths>,
) -> Result<Router> {
    let images = renderer.images.clone();
    let endpoint = graphql::endpoint_with_graphiql(catalog.clone(), renderer, graphiql.is_some())?;
    let assets = Assets {
        root: catalog.assets.clone(),
        workers: Arc::new(Semaphore::new(4)),
    };
    let mut router = Router::new();
    if let Some(paths) = graphiql {
        router = router.merge(crate::graphiql::routes(
            endpoint.graphiql().expect("enabled GraphiQL"),
            paths,
        ));
    }
    let router = router
        .merge(
            Router::new()
                .route("/images/{width}/{*path}", get(crate::images::http::image))
                .with_state(images),
        )
        .route_service("/graphql", chigiri_axum::service(endpoint)?)
        .route("/assets/{*path}", get(asset))
        .with_state(assets)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_headers(Any)
                .allow_methods([Method::GET, Method::HEAD, Method::POST, Method::OPTIONS])
                .expose_headers([header::ETAG, header::CONTENT_TYPE]),
        );
    Ok(router)
}

async fn asset(
    State(assets): State<Assets>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    method: Method,
) -> Response {
    let path = uri.path().strip_prefix("/assets/").unwrap_or("");
    if path.len() > 8192 {
        return StatusCode::URI_TOO_LONG.into_response();
    }
    let Ok(path) = percent_decode_str(path).decode_utf8() else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let segments: Vec<&str> = path.split('/').collect();
    if segments
        .iter()
        .any(|part| part.is_empty() || *part == "." || *part == ".." || part.contains(['\\', '\0']))
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let mime = mime_guess::from_path(path.as_ref())
        .first_or_octet_stream()
        .to_string();
    let Ok(permit) = assets.workers.acquire_owned().await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let path = path.into_owned();
    let loaded = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let segments: Vec<&str> = path.split('/').collect();
        let root = assets.root.open()?;
        let result = storage::read_asset(&root, &segments, 32 * 1024 * 1024)?;
        assets.root.validate(&root)?;
        Ok::<_, anyhow::Error>(result)
    })
    .await;
    let (bytes, hash) = match loaded {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            tracing::debug!(event="asset_unavailable", error=%format!("{error:#}"));
            let interrupted = error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::Interrupted);
            return if interrupted {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    [(header::CACHE_CONTROL, "no-store")],
                )
                    .into_response()
            } else {
                StatusCode::NOT_FOUND.into_response()
            };
        }
        Err(error) => {
            tracing::error!(event="asset_worker_failed", %error);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    use std::fmt::Write;
    let mut etag = String::from("\"");
    for byte in hash {
        write!(etag, "{byte:02x}").expect("string write");
    }
    etag.push('"');
    let unchanged = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(',').any(|candidate| {
                candidate.trim().trim_start_matches("W/") == etag || candidate.trim() == "*"
            })
        });
    let length = bytes.len();
    let mut response = if unchanged {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        let mut response = Response::new(if method == Method::HEAD {
            Body::empty()
        } else {
            Body::from(bytes)
        });
        response
            .headers_mut()
            .insert(header::CONTENT_LENGTH, HeaderValue::from(length));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_str(&mime).expect("valid MIME"),
        );
        response
    };
    response.headers_mut().insert(
        header::ETAG,
        HeaderValue::from_str(&etag).expect("valid etag"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

pub async fn serve(config: Config) -> Result<()> {
    let (catalog, reload) = crate::reload::Reload::start(&config).await?;
    let config = Arc::new(config);
    let renderer = Renderer::new(config.clone(), &catalog);
    let images = renderer.images.clone();
    let router = router_with_config(catalog.clone(), renderer, &config)?;
    let listener = match tokio::net::TcpListener::bind(config.bind).await {
        Ok(listener) => listener,
        Err(error) => {
            reload.shutdown().await?;
            return Err(error.into());
        }
    };
    tracing::info!(event="startup", bind=%listener.local_addr()?, public_base=%config.public_base, graphiql=config.graphiql, watch=config.watch, root=%config.root.display(), articles=catalog.snapshot().articles.len());
    use std::future::IntoFuture;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let (stop, mut stopped) = tokio::sync::watch::channel(false);
    let server = axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = stopped.changed().await;
        })
        .into_future();
    tokio::pin!(server);
    tokio::select! {
        result = &mut server => { reload.shutdown().await?; result?; }
        _ = async { tokio::select! { _ = terminate.recv() => {}, _ = interrupt.recv() => {} } } => {
            tracing::info!(event="shutdown_requested");
            images.begin_shutdown();
            stop.send_replace(true);
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
            match tokio::time::timeout_at(deadline, reload.shutdown()).await {
                Ok(result) => result?,
                Err(_) => tracing::warn!(event="shutdown_reload_timeout", "reload drain exceeded 30 seconds"),
            }
            match tokio::time::timeout_at(deadline, &mut server).await {
                Ok(result) => result?,
                Err(_) => tracing::warn!(event="shutdown_drain_timeout", "HTTP drain exceeded 30 seconds"),
            }
        }
    }
    tracing::info!(event = "shutdown");
    Ok(())
}
