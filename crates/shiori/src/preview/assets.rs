use crate::storage;
use anyhow::{Result, bail, ensure};
use rustix::fs::{AtFlags, FileType, statat};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    os::unix::fs::MetadataExt,
};

fn identity(file: &File) -> Result<(u64, u64, u64, i64, i64, i64, i64)> {
    let m = file.metadata()?;
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

fn hash_file(parent: &File, name: &str) -> Result<[u8; 32]> {
    for _ in 0..3 {
        let mut file = storage::regular(parent, name)?;
        let before = identity(&file)?;
        let mut first = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let length = file.read(&mut buffer)?;
            if length == 0 {
                break;
            }
            first.update(&buffer[..length]);
        }
        file.seek(SeekFrom::Start(0))?;
        let mut second = Sha256::new();
        loop {
            let length = file.read(&mut buffer)?;
            if length == 0 {
                break;
            }
            second.update(&buffer[..length]);
        }
        let hash: [u8; 32] = first.finalize().into();
        if hash == <[u8; 32]>::from(second.finalize())
            && before == identity(&file)?
            && before == identity(&storage::regular(parent, name)?)?
        {
            return Ok(hash);
        }
    }
    bail!("asset changed during scan")
}

fn directory(parent: &File, digest: &mut Sha256, depth: usize) -> Result<()> {
    ensure!(depth <= 64, "assets nesting exceeds 64 directories");
    let mut entries = Vec::new();
    for entry in rustix::fs::Dir::read_from(parent)? {
        let entry = entry?;
        let name = entry.file_name().to_bytes();
        if name == b"." || name == b".." {
            continue;
        }
        entries.push(std::str::from_utf8(name)?.to_owned());
    }
    entries.sort();
    for name in entries {
        let stat = statat(parent, name.as_str(), AtFlags::SYMLINK_NOFOLLOW)?;
        match FileType::from_raw_mode(stat.st_mode) {
            FileType::RegularFile => {
                digest.update(b"file");
                digest.update((name.len() as u64).to_be_bytes());
                digest.update(name.as_bytes());
                digest.update(hash_file(parent, &name)?);
            }
            FileType::Directory => {
                digest.update(b"dir");
                digest.update((name.len() as u64).to_be_bytes());
                digest.update(name.as_bytes());
                directory(&storage::directory(parent, &name)?, digest, depth + 1)?;
                digest.update(b"end");
            }
            _ => {} // Public content lookup never follows symlinks or special files.
        }
    }
    Ok(())
}

pub(super) fn fingerprint(root: &File) -> Result<[u8; 32]> {
    let mut digest = Sha256::new();
    match storage::directory(root, "assets") {
        Ok(assets) => directory(&assets, &mut digest, 0)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(digest.finalize().into())
}
