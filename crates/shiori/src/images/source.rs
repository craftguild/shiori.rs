use super::{Error, MAX_BYTES};
use crate::storage;
use percent_encoding::percent_decode_str;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    os::unix::fs::MetadataExt,
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct AssetPath(pub String);
impl AssetPath {
    pub fn parse(encoded: &str) -> Result<Self, Error> {
        if encoded.len() > 8192 {
            return Err(Error::Path);
        }
        let path = percent_decode_str(encoded)
            .decode_utf8()
            .map_err(|_| Error::Path)?;
        if path
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == ".." || s.contains(['\\', '\0']))
        {
            return Err(Error::Path);
        }
        Ok(Self(path.into_owned()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SourceState {
    Missing,
    Hash([u8; 32]),
    Unstable,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Dependency {
    pub path: AssetPath,
    pub state: SourceState,
}
impl Dependency {
    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.path.0.capacity()
    }
}

fn open(root: &File, path: &AssetPath) -> Result<File, Error> {
    let mut parts = path.0.split('/').peekable();
    let mut dir = root.try_clone().map_err(|_| Error::Missing)?;
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            return storage::regular(&dir, part).map_err(|_| Error::Missing);
        }
        dir = storage::directory(&dir, part).map_err(|_| Error::Missing)?;
    }
    Err(Error::Path)
}
type Fingerprint = (u64, u64, u64, i64, i64, i64, i64);
fn fingerprint(file: &File) -> Result<Fingerprint, Error> {
    let m = file.metadata().map_err(|_| Error::Unstable)?;
    Ok((
        m.dev(),
        m.ino(),
        m.len(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
    ))
}
fn bounded(file: &mut File) -> Result<Vec<u8>, Error> {
    let mut data = Vec::new();
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut data)
        .map_err(|_| Error::Unstable)?;
    if data.len() > MAX_BYTES {
        return Err(Error::Missing);
    }
    Ok(data)
}
pub(super) fn read(
    source: &crate::asset_root::AssetRoot,
    path: &AssetPath,
) -> Result<(Vec<u8>, [u8; 32]), Error> {
    let root = source.open().map_err(|_| Error::DirectoryChanged)?;
    for _ in 0..3 {
        let mut file = open(&root, path)?;
        let before = fingerprint(&file)?;
        if before.2 > MAX_BYTES as u64 {
            return Err(Error::Missing);
        }
        let bytes = bounded(&mut file)?;
        file.seek(SeekFrom::Start(0)).map_err(|_| Error::Unstable)?;
        let second = bounded(&mut file)?;
        if bytes == second
            && before == fingerprint(&file)?
            && before == fingerprint(&open(&root, path)?)?
        {
            source
                .validate(&root)
                .map_err(|_| Error::DirectoryChanged)?;
            return Ok((bytes, Sha256::digest(second).into()));
        }
    }
    Err(Error::Unstable)
}
pub(super) fn state(root: &crate::asset_root::AssetRoot, path: &AssetPath) -> SourceState {
    match read(root, path) {
        Ok((_, hash)) => SourceState::Hash(hash),
        Err(Error::Missing) => SourceState::Missing,
        _ => SourceState::Unstable,
    }
}
