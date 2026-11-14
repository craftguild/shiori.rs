use reqwest::blocking::{Client, Response};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

pub fn binary() -> PathBuf {
    PathBuf::from(
        std::env::var_os("SHIORI_ACCEPTANCE_BINARY")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_shiori").into()),
    )
    .canonicalize()
    .unwrap()
}

pub fn command(arguments: &[&str]) -> std::process::Output {
    Command::new(binary()).args(arguments).output().unwrap()
}

pub struct Server {
    child: Child,
    events: Receiver<Value>,
    reader: Option<thread::JoinHandle<()>>,
    pub origin: String,
    pub client: Client,
}

impl Server {
    pub fn start(root: &Path, host: &str, public_base: Option<&str>, graphiql: bool) -> Self {
        let listener = TcpListener::bind((host, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let origin = format!("http://{address}");
        let mut command = Command::new(binary());
        command.args([
            "--root",
            root.to_str().unwrap(),
            "serve",
            "--bind",
            &address.to_string(),
            "--public-base-url",
            public_base.unwrap_or(&origin),
        ]);
        if graphiql {
            command.arg("--graphiql");
        }
        let mut child = command
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stderr = child.stderr.take().unwrap();
        let (sender, events) = mpsc::channel();
        let reader = thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                let line = line.unwrap();
                eprintln!("{line}");
                if let Ok(value) = serde_json::from_str::<Value>(&line) {
                    let _ = sender.send(value);
                }
            }
        });
        let client = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap();
        let mut server = Self {
            child,
            events,
            reader: Some(reader),
            origin,
            client,
        };
        server.event("startup");
        server
    }

    pub fn event(&mut self, name: &str) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let event = self
                .events
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|error| {
                    panic!(
                        "No {name} event: {error}; child {:?}",
                        self.child.try_wait()
                    )
                });
            if event["fields"]["event"] == name {
                return;
            }
        }
    }

    pub fn request(&self, path: &str) -> Response {
        self.client
            .get(format!("{}{path}", self.origin))
            .send()
            .unwrap()
    }

    pub fn query(&self, query: &str, get: bool) -> Value {
        let url = format!("{}/graphql", self.origin);
        let request = if get {
            self.client.get(url).query(&[("query", query)])
        } else {
            self.client.post(url).json(&json!({"query": query}))
        };
        let response = request
            .header("Origin", "https://reader.example")
            .header("x-chigiri-preflight", "1")
            .send()
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["access-control-allow-origin"], "*");
        response.json().unwrap()
    }

    pub fn close(mut self) {
        self.stop(true);
    }

    fn stop(&mut self, require_success: bool) {
        if self.child.try_wait().unwrap().is_none() {
            // Only signal this owned, still-unreaped process.
            assert!(
                Command::new("kill")
                    .args(["-TERM", &self.child.id().to_string()])
                    .status()
                    .unwrap()
                    .success()
            );
            let deadline = Instant::now() + Duration::from_secs(65);
            while self.child.try_wait().unwrap().is_none() {
                if Instant::now() > deadline {
                    self.child.kill().unwrap();
                    self.child.wait().unwrap();
                    panic!("Server did not drain within its documented bound");
                }
                thread::sleep(Duration::from_millis(20));
            }
        }
        let status = self.child.wait().unwrap();
        if let Some(reader) = self.reader.take() {
            reader.join().unwrap();
        }
        if require_success {
            assert!(status.success(), "Server exited {status}");
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if self.reader.is_some() {
            self.stop(false);
        }
    }
}

pub mod proxy;

/// Restore fixture permissions even if a response or browser assertion panics.
pub struct ReadOnlyRoot(Vec<PathBuf>);

impl ReadOnlyRoot {
    pub fn new(root: &Path) -> Self {
        use std::os::unix::fs::PermissionsExt;
        fn collect(path: &Path, paths: &mut Vec<PathBuf>) {
            paths.push(path.to_owned());
            if path.is_dir() {
                for child in std::fs::read_dir(path).unwrap() {
                    collect(&child.unwrap().path(), paths);
                }
            }
        }
        let mut paths = Vec::new();
        collect(root, &mut paths);
        let guard = Self(paths);
        for path in &guard.0 {
            std::fs::set_permissions(
                path,
                std::fs::Permissions::from_mode(if path.is_dir() { 0o555 } else { 0o444 }),
            )
            .unwrap();
        }
        guard
    }
}

impl Drop for ReadOnlyRoot {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        for path in &self.0 {
            std::fs::set_permissions(
                path,
                std::fs::Permissions::from_mode(if path.is_dir() { 0o755 } else { 0o644 }),
            )
            .unwrap();
        }
    }
}
