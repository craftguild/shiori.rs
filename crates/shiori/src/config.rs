use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use url::Url;

#[derive(Clone, Debug)]
pub struct Config {
    pub root: PathBuf,
    pub bind: SocketAddr,
    pub public_base: Url,
    pub graphiql: bool,
    pub watch: bool,
    pub layout: crate::layout::Layout,
}

impl Config {
    pub fn new(root: &Path, bind: SocketAddr, public_base: &str) -> Result<Self> {
        Self::with_layout(root, bind, public_base, crate::layout::Layout::from_env()?)
    }

    pub fn with_layout(
        root: &Path,
        bind: SocketAddr,
        public_base: &str,
        layout: crate::layout::Layout,
    ) -> Result<Self> {
        let root = root.canonicalize().context("root must already exist")?;
        ensure!(root.is_dir(), "root must be a directory");
        let mut public_base = Url::parse(public_base).context("invalid public base URL")?;
        ensure!(
            matches!(public_base.scheme(), "http" | "https") && public_base.host_str().is_some(),
            "public base URL must be absolute HTTP(S)"
        );
        ensure!(
            public_base.username().is_empty()
                && public_base.password().is_none()
                && public_base.query().is_none()
                && public_base.fragment().is_none(),
            "public base URL cannot contain userinfo, query or fragment"
        );
        ensure!(
            public_base.as_str().len() <= 4096,
            "public base URL exceeds 4096 bytes"
        );
        if !public_base.path().ends_with('/') {
            public_base.set_path(&format!("{}/", public_base.path()));
        }
        layout.create(&crate::storage::root(&root)?)?;
        Ok(Self {
            root,
            bind,
            public_base,
            graphiql: false,
            watch: false,
            layout,
        })
    }

    pub fn image_url(&self, destination: &str) -> Option<String> {
        destination.strip_prefix("/assets/").map(|tail| {
            // Concatenation preserves the producer's query/fragment and encoded path;
            // only the parsed, canonical local-image prefix is transformed.
            format!("{}assets/{tail}", self.public_base)
        })
    }
}
