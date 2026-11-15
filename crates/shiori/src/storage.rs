//! Directory-handle operations keep every lookup beneath the configured root.
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    os::unix::fs::MetadataExt,
    path::Path,
};

use anyhow::{Context, Result, bail, ensure};
use rustix::fs::{Mode, OFlags, openat};

use crate::metadata::MAX_FILE_BYTES;

pub fn directory(parent: &File, name: &str) -> std::io::Result<File> {
    Ok(openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?
    .into())
}

pub fn root(path: &Path) -> Result<File> {
    let file = File::open(path).with_context(|| format!("cannot open root {}", path.display()))?;
    ensure!(file.metadata()?.is_dir(), "root is not a directory");
    Ok(file)
}

pub fn regular(parent: &File, name: &str) -> std::io::Result<File> {
    let file: File = openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )?
    .into();
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    Ok(file)
}

pub(crate) fn fingerprint(meta: &std::fs::Metadata) -> (u64, u64, u64, i64, i64, i64, i64) {
    (
        meta.dev(),
        meta.ino(),
        meta.len(),
        meta.mtime(),
        meta.mtime_nsec(),
        meta.ctime(),
        meta.ctime_nsec(),
    )
}

fn bounded(file: &mut File, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= limit, "file exceeds {limit} bytes");
    Ok(bytes)
}

pub fn stable_read(parent: &File, name: &str) -> Result<Vec<u8>> {
    Ok(stable_source(parent, name)?.0)
}

pub fn source_parent(parent: &File, path: &str) -> Result<(File, String)> {
    let mut parts = path.split('/').peekable();
    let mut dir = parent.try_clone()?;
    while let Some(part) = parts.next() {
        ensure!(
            !part.is_empty() && part != "." && part != ".." && !part.contains(['\\', '\0']),
            "invalid source path"
        );
        if parts.peek().is_none() {
            return Ok((dir, part.to_owned()));
        }
        dir = directory(&dir, part)?;
    }
    bail!("empty source path")
}

pub fn stable_source(parent: &File, path: &str) -> Result<(Vec<u8>, std::fs::Metadata)> {
    let (parent, name) = source_parent(parent, path)?;
    for _ in 0..3 {
        let mut file = regular(&parent, &name)?;
        let before = fingerprint(&file.metadata()?);
        ensure!(
            before.2 <= MAX_FILE_BYTES as u64,
            "source exceeds {MAX_FILE_BYTES} bytes"
        );
        let first = bounded(&mut file, MAX_FILE_BYTES)?;
        file.seek(SeekFrom::Start(0))?;
        let second = bounded(&mut file, MAX_FILE_BYTES)?;
        let meta = file.metadata()?;
        let after = fingerprint(&meta);
        let current = regular(&parent, &name)?;
        if before == after && after == fingerprint(&current.metadata()?) && first == second {
            return Ok((first, meta));
        }
    }
    bail!("file changed during three bounded read attempts; retry on next reload")
}

pub fn read_asset(root: &File, segments: &[&str], limit: usize) -> Result<(Vec<u8>, [u8; 32])> {
    use sha2::{Digest, Sha256};
    let mut dir = root.try_clone()?;
    for segment in &segments[..segments.len() - 1] {
        dir = directory(&dir, segment)?;
    }
    let mut file = regular(&dir, segments[segments.len() - 1])?;
    ensure!(
        file.metadata()?.len() <= limit as u64,
        "asset exceeds {limit} bytes"
    );
    let bytes = bounded(&mut file, limit)?;
    let hash = Sha256::digest(&bytes).into();
    Ok((bytes, hash))
}
