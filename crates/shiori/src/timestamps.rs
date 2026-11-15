use anyhow::{Context, Result, ensure};
use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashSet},
    fs::Metadata,
    path::{Path, PathBuf},
    time::SystemTime,
};
use time::{OffsetDateTime, macros::format_description};

#[cfg(test)]
mod tests;

pub fn format(date: OffsetDateTime) -> Result<String> {
    if !(1..=9999).contains(&date.year()) {
        return Err(UnsupportedDate.into());
    }
    Ok(date.format(format_description!(
        "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
    ))?)
}

#[derive(Debug)]
pub struct UnsupportedDate;
impl std::fmt::Display for UnsupportedDate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("timestamp must be within UTC years 1 through 9999")
    }
}
impl std::error::Error for UnsupportedDate {}

fn from_seconds(seconds: i64) -> Result<OffsetDateTime> {
    OffsetDateTime::from_unix_timestamp(seconds).map_err(|_| UnsupportedDate.into())
}

fn from_system(value: SystemTime) -> Result<OffsetDateTime> {
    let nanos = match value.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(duration) => duration.as_nanos() as i128,
        Err(error) => -(error.duration().as_nanos() as i128),
    };
    OffsetDateTime::from_unix_timestamp_nanos(nanos).map_err(|_| UnsupportedDate.into())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dates {
    pub created: String,
    pub updated: String,
    pub created_ms: i64,
}

impl Dates {
    fn new(created: OffsetDateTime, updated: OffsetDateTime) -> Result<Self> {
        Ok(Self {
            created: format(created)?,
            updated: format(updated)?,
            created_ms: i64::try_from(created.unix_timestamp_nanos().div_euclid(1_000_000))?,
        })
    }
    pub fn filesystem(meta: &Metadata) -> Result<Self> {
        let updated = meta.modified().context("cannot read modification time")?;
        Self::system(meta.created().unwrap_or(updated), updated)
    }
    pub fn system(created: SystemTime, updated: SystemTime) -> Result<Self> {
        Self::new(from_system(created)?, from_system(updated)?)
    }
}

pub struct SourceDates {
    repo: Option<gix::Repository>,
    head: Option<gix::ObjectId>,
    pub token: Option<String>,
    prefix: PathBuf,
}

impl SourceDates {
    pub fn open(root: Option<&Path>) -> Result<Self> {
        let mut result = Self {
            repo: None,
            head: None,
            token: None,
            prefix: PathBuf::new(),
        };
        let Some(root) = root else { return Ok(result) };
        // A missing repository is ordinary filesystem mode; a broken .git is an error.
        let mut present = false;
        for parent in root.ancestors() {
            match std::fs::symlink_metadata(parent.join(".git")) {
                Ok(_) => {
                    present = true;
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error).context("cannot inspect Git worktree boundary"),
            }
        }
        if !present {
            return Ok(result);
        }
        let repo = gix::discover(root).context("cannot discover Git worktree")?;
        ensure!(
            !repo.is_shallow()?,
            "Git history is shallow; complete history is required"
        );
        let worktree = repo
            .workdir()
            .context("document root requires a Git worktree")?
            .canonicalize()?;
        result.prefix = root.strip_prefix(worktree)?.to_path_buf();
        let head = repo.head()?;
        result.head = head.id().map(|id| id.detach());
        result.token = Some(format!(
            "{}:{}",
            repo.path().display(),
            result
                .head
                .map_or_else(|| "unborn".to_owned(), |id| id.to_string())
        ));
        drop(head);
        result.repo = Some(repo);
        Ok(result)
    }

    pub fn source_path(&self, relative: &Path) -> Result<String> {
        Ok(self
            .prefix
            .join(relative)
            .to_str()
            .context("source path must be UTF-8")?
            .to_owned())
    }

    pub fn committed(&self, path: &str) -> Result<Option<Dates>> {
        let (Some(repo), Some(head)) = (&self.repo, self.head) else {
            return Ok(None);
        };
        let head_commit = repo.find_commit(head).context("cannot read Git HEAD")?;
        if entry(&head_commit, path)?.is_none() {
            return Ok(None);
        }
        let mut pending = BinaryHeap::new();
        let mut visited = HashSet::new();
        let mut sequence = 0_u64;
        pending.push((head_commit.time()?.seconds, Reverse(sequence), head));
        let mut updated = None;
        while let Some((seconds, _, id)) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }
            let commit = repo
                .find_commit(id)
                .context("cannot read Git commit history")?;
            let state = entry(&commit, path)?;
            let mut parents = Vec::new();
            for parent in commit.parent_ids() {
                let parent_commit = parent.object()?.try_into_commit()?;
                parents.push((
                    parent.detach(),
                    entry(&parent_commit, path)?,
                    parent_commit.time()?.seconds,
                ));
            }
            // Default Git path-history simplification follows one TREESAME
            // parent. A merge changed against every parent explores all of them.
            if let Some(parent) = parents
                .iter()
                .find(|(_, previous, _)| previous == &state)
                .cloned()
            {
                parents = vec![parent];
            } else {
                let date = from_seconds(seconds)?;
                updated.get_or_insert(date);
                if parents.iter().all(|(_, state, _)| state.is_none()) {
                    return Ok(Some(Dates::new(date, updated.expect("changed path"))?));
                }
            }
            for (parent, state, seconds) in parents {
                // Stop at the current path lifetime; never cross deletion or rename.
                if state.is_some() {
                    sequence += 1;
                    pending.push((seconds, Reverse(sequence), parent));
                }
            }
        }
        anyhow::bail!("Git history has no addition for the current path")
    }

    pub fn verify(&self, root: Option<&Path>) -> Result<()> {
        ensure!(
            Self::open(root)?.token == self.token,
            "Git HEAD changed during catalog scan; retry"
        );
        Ok(())
    }
}

fn entry(commit: &gix::Commit<'_>, path: &str) -> Result<Option<(gix::ObjectId, u16)>> {
    let tree = commit.tree()?;
    let Some(entry) = tree.lookup_entry_by_path(path)? else {
        return Ok(None);
    };
    ensure!(
        entry.mode().is_blob(),
        "Git article source is not a regular blob"
    );
    Ok(Some((entry.object_id(), entry.mode().value())))
}
