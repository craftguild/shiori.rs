use crate::storage;
use std::{
    fs::File,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
};

/// A small, process-local directory reference with cancel-and-revalidate semantics.
pub(crate) struct AssetRoot {
    root: Arc<File>,
    path: PathBuf,
    current: Mutex<File>,
}

impl AssetRoot {
    pub(crate) fn new(root: Arc<File>, path: PathBuf) -> std::io::Result<Self> {
        let current = Self::resolve(&root, &path)?;
        Ok(Self {
            root,
            path,
            current: Mutex::new(current),
        })
    }

    pub(crate) fn open(&self) -> std::io::Result<File> {
        use std::os::unix::fs::MetadataExt;
        let next = Self::resolve(&self.root, &self.path).map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                format!("asset directory unavailable; retry: {error}"),
            )
        })?;
        let metadata = next.metadata()?;
        let mut current = self.current.lock().expect("asset root lock poisoned");
        let previous = current.metadata()?;
        if metadata.dev() != previous.dev() || metadata.ino() != previous.ino() {
            *current = next;
            tracing::warn!(
                event = "asset_directory_changed",
                "operation cancelled; next operation revalidates the configured directory"
            );
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "asset directory changed; retry",
            ));
        }
        current.try_clone()
    }

    pub(crate) fn validate(&self, pinned: &File) -> std::io::Result<()> {
        use std::os::unix::fs::MetadataExt;
        let current = self.open()?;
        let actual = current.metadata()?;
        let selected = pinned.metadata()?;
        if actual.dev() == selected.dev() && actual.ino() == selected.ino() {
            return Ok(());
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::Interrupted,
            "asset directory changed during operation; retry",
        ))
    }

    fn resolve(root: &File, path: &Path) -> std::io::Result<File> {
        let mut directory = root.try_clone()?;
        for part in path.components() {
            let Component::Normal(name) = part else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "invalid asset directory",
                ));
            };
            let name = name.to_str().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "asset directory must be UTF-8",
                )
            })?;
            directory = storage::directory(&directory, name)?;
        }
        Ok(directory)
    }
}
