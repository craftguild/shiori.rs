use crate::storage;
use anyhow::{Context, Result, ensure};
use rustix::fs::{Mode, mkdirat};
use std::{
    fs::File,
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct Layout {
    pub articles: PathBuf,
    pub assets: PathBuf,
}

fn relative(value: &str) -> Result<PathBuf> {
    ensure!(
        !value.is_empty() && !value.contains(['\\', '\0']),
        "invalid storage directory"
    );
    let mut path = PathBuf::new();
    for part in Path::new(value).components() {
        match part {
            Component::CurDir => {}
            Component::Normal(name) => {
                ensure!(name.to_str().is_some(), "directory must be UTF-8");
                path.push(name);
            }
            _ => anyhow::bail!("storage directories must stay relative to root"),
        }
    }
    ensure!(
        !path.as_os_str().is_empty(),
        "storage directory cannot be root"
    );
    Ok(path)
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            articles: "articles".into(),
            assets: "assets".into(),
        }
    }
}

impl Layout {
    pub fn new(articles: &str, assets: &str) -> Result<Self> {
        let layout = Self {
            articles: relative(articles)?,
            assets: relative(assets)?,
        };
        ensure!(
            !layout.articles.starts_with(&layout.assets)
                && !layout.assets.starts_with(&layout.articles),
            "article and asset directories must not overlap"
        );
        Ok(layout)
    }
    pub fn from_env() -> Result<Self> {
        let articles = std::env::var("SHIORI_ARTICLES_DIR")
            .context("SHIORI_ARTICLES_DIR must be Unicode")
            .or_else(|e| {
                if std::env::var_os("SHIORI_ARTICLES_DIR").is_none() {
                    Ok("./articles".to_owned())
                } else {
                    Err(e)
                }
            })?;
        let assets = std::env::var("SHIORI_ASSETS_DIR")
            .context("SHIORI_ASSETS_DIR must be Unicode")
            .or_else(|e| {
                if std::env::var_os("SHIORI_ASSETS_DIR").is_none() {
                    Ok("./assets".to_owned())
                } else {
                    Err(e)
                }
            })?;
        Self::new(&articles, &assets)
    }
    pub fn create(&self, root: &File) -> Result<()> {
        self.open(root, &self.articles, true)
            .context("cannot prepare article directory")?;
        self.open(root, &self.assets, true)
            .context("cannot prepare asset directory")?;
        Ok(())
    }
    pub fn open(&self, root: &File, path: &Path, create: bool) -> Result<File> {
        let mut dir = root.try_clone()?;
        for part in path.components() {
            let Component::Normal(name) = part else {
                anyhow::bail!("invalid relative directory")
            };
            let name = name.to_str().context("directory must be UTF-8")?;
            dir = match storage::directory(&dir, name) {
                Ok(opened) => opened,
                Err(error) if create && error.kind() == std::io::ErrorKind::NotFound => {
                    if let Err(error) = mkdirat(&dir, name, Mode::from_raw_mode(0o755))
                        && error != rustix::io::Errno::EXIST
                    {
                        return Err(error.into());
                    }
                    storage::directory(&dir, name)?
                }
                Err(error) => return Err(error.into()),
            };
        }
        Ok(dir)
    }
}
