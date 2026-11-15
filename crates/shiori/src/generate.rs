use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail, ensure};
use rustix::fs::{AtFlags, Mode, OFlags, linkat, mkdirat, openat, unlinkat};

use crate::{
    metadata::{Metadata, parse_id},
    storage,
};

struct AssetDirectory<'a> {
    parent: &'a File,
    name: String,
    published: bool,
}
impl Drop for AssetDirectory<'_> {
    fn drop(&mut self) {
        if !self.published {
            // Only remove our empty reservation, never another writer's files.
            let _ = unlinkat(self.parent, self.name.as_str(), AtFlags::REMOVEDIR);
        }
    }
}

struct Temporary<'a> {
    dir: &'a File,
    name: String,
}
impl Drop for Temporary<'_> {
    fn drop(&mut self) {
        let _ = unlinkat(self.dir, self.name.as_str(), AtFlags::empty());
    }
}

pub fn generate(root: &Path, title: &str) -> Result<PathBuf> {
    generate_with_clock(root, title, || {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("clock is before Unix epoch")?;
        Ok(now.as_millis().to_string())
    })
}

/// The injectable clock permits deterministic collision/error checks without fabricated IDs.
pub fn generate_with_clock(
    root: &Path,
    title: &str,
    mut clock: impl FnMut() -> Result<String>,
) -> Result<PathBuf> {
    let title = title.trim();
    ensure!(!title.is_empty(), "title cannot be empty");
    ensure!(title.len() <= 32 * 1024, "title exceeds 32 KiB");
    let root = root.canonicalize().context("root must already exist")?;
    let root_fd = storage::root(&root)?;
    let layout = crate::layout::Layout::from_env()?;
    layout.create(&root_fd)?;
    let dir = layout.open(&root_fd, &layout.articles, false)?;
    let assets = layout.open(&root_fd, &layout.assets, false)?;
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let name = format!(
        ".shiori-{}-{}.tmp",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    let mut file: File = openat(
        &dir,
        name.as_str(),
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o644),
    )
    .context("cannot create temporary article")?
    .into();
    let temporary = Temporary { dir: &dir, name };
    let yaml = serde_saphyr::to_string(&Metadata {
        title: title.to_owned(),
        author: String::new(),
        description: String::new(),
        tags: vec![],
    })?;
    ensure!(
        yaml.len() <= crate::metadata::MAX_FRONT_MATTER,
        "serialized metadata exceeds 64 KiB; shorten the title"
    );
    write!(file, "---\n{yaml}---\n\n").context("cannot write temporary article")?;
    file.flush()?;
    file.sync_all().context("cannot flush temporary article")?;
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let id = clock()?;
        parse_id(&id).context("clock did not produce a supported epoch timestamp")?;
        let destination = format!("{id}.md");
        if rustix::fs::statat(
            &dir,
            format!("{id}.markdown").as_str(),
            AtFlags::SYMLINK_NOFOLLOW,
        )
        .is_ok()
        {
            ensure!(
                Instant::now() < deadline,
                "timestamp collision persisted for one second"
            );
            std::thread::sleep(Duration::from_millis(1));
            continue;
        }
        match mkdirat(&assets, id.as_str(), Mode::from_raw_mode(0o755)) {
            Ok(()) => {}
            Err(rustix::io::Errno::EXIST) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
            Err(rustix::io::Errno::EXIST) => {
                bail!("timestamp collision persisted for one second; no assets overwritten")
            }
            Err(error) => return Err(error).context("cannot reserve article assets directory"),
        }
        let mut asset_directory = AssetDirectory {
            parent: &assets,
            name: id,
            published: false,
        };
        assets
            .sync_all()
            .context("cannot flush reserved assets directory")?;
        match linkat(
            &dir,
            temporary.name.as_str(),
            &dir,
            destination.as_str(),
            AtFlags::empty(),
        ) {
            Ok(()) => {
                asset_directory.published = true;
                unlinkat(&dir, temporary.name.as_str(), AtFlags::empty())
                    .context("article published but temporary cleanup failed")?;
                dir.sync_all()
                    .context("article published but directory flush failed")?;
                return Ok(root.join(&layout.articles).join(destination));
            }
            Err(rustix::io::Errno::EXIST) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(rustix::io::Errno::EXIST) => {
                bail!("timestamp collision persisted for one second; no article overwritten")
            }
            Err(error) => {
                return Err(error).context("cannot atomically publish article without overwrite");
            }
        }
    }
}
