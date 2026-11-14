//! A real prefix-stripping proxy, isolated to the release fixture.
use axum::{Router, body::Body, extract::State, http::Request, response::Response, routing::any};
use std::{
    sync::{Arc, RwLock},
    thread,
};
use tokio::sync::oneshot;

type Target = Arc<RwLock<String>>;

pub struct Proxy {
    pub origin: String,
    target: Target,
    stop: Option<oneshot::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Proxy {
    pub fn start() -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let target = Arc::new(RwLock::new(String::new()));
        let state = target.clone();
        let (stop, stopped) = oneshot::channel();
        let thread = thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                    let router = Router::new()
                        .route("/blog/v1/{*path}", any(forward))
                        .with_state(state);
                    axum::serve(listener, router)
                        .with_graceful_shutdown(async {
                            let _ = stopped.await;
                        })
                        .await
                        .unwrap();
                });
        });
        Self {
            origin,
            target,
            stop: Some(stop),
            thread: Some(thread),
        }
    }

    pub fn target(&self, origin: &str) {
        *self.target.write().unwrap() = origin.to_owned();
    }
}

async fn forward(State(target): State<Target>, request: Request<Body>) -> Response {
    let origin = target.read().unwrap().clone();
    let (parts, body) = request.into_parts();
    let path = parts.uri.to_string();
    let url = format!("{origin}{}", path.strip_prefix("/blog/v1").unwrap());
    let bytes = axum::body::to_bytes(body, 256 * 1024).await.unwrap();
    tokio::task::spawn_blocking(move || {
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .build()
            .unwrap();
        let mut request = client.request(parts.method, url);
        for (name, value) in &parts.headers {
            if !["host", "connection", "content-length"].contains(&name.as_str()) {
                request = request.header(name, value);
            }
        }
        let response = request.body(bytes).send().unwrap();
        let mut result = Response::builder().status(response.status());
        for (name, value) in response.headers() {
            if !["transfer-encoding", "connection", "server", "date"].contains(&name.as_str()) {
                result = result.header(name, value);
            }
        }
        result.body(Body::from(response.bytes().unwrap())).unwrap()
    })
    .await
    .unwrap()
}

impl Drop for Proxy {
    fn drop(&mut self) {
        let _ = self.stop.take().unwrap().send(());
        self.thread.take().unwrap().join().unwrap();
    }
}
