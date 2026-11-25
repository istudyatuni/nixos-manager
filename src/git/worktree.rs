use std::path::Path;

use tokio::fs;

use crate::{command_runner::CommandRunner, config::Config};

use super::{
    Error,
    command::{command, run},
    remote::matching_remote,
};

#[derive(Clone, Default)]
pub struct WorkingCopy {
    pub branch: Option<String>,
    pub commit: Option<String>,
    pub selected_commit: Option<String>,
    pub has_local_changes: bool,
    pub matches_revision: bool,
}

impl WorkingCopy {
    pub fn clean_head(&self) -> Option<&str> {
        self.commit
            .as_deref()
            .filter(|_| self.matches_revision && !self.has_local_changes)
    }
}

pub async fn inspect(runner: &CommandRunner, config: &Config) -> Result<WorkingCopy, Error> {
    let path = &config.path;
    check_worktree(runner, path, &config.repository).await?;
    let has_local_changes = worktree_dirty(runner, path).await?;
    let current_branch = current_branch(runner, path).await.ok();
    let head = revision(runner, path, "HEAD").await.ok();
    let commit = head.as_deref().map(commit_id);
    let selected_commit = tracking_commit(runner, path, &tracking_ref(&config.branch)).await;
    let matches_revision = current_branch.as_deref() == Some(config.branch.as_bytes())
        && commit.is_some()
        && commit == selected_commit;
    Ok(WorkingCopy {
        branch: current_branch.map(|branch| String::from_utf8_lossy(&branch).into_owned()),
        commit,
        selected_commit,
        has_local_changes,
        matches_revision,
    })
}

pub async fn check_worktree(
    runner: &CommandRunner,
    path: &Path,
    repository: &str,
) -> Result<String, Error> {
    if !fs::metadata(path)
        .await
        .is_ok_and(|metadata| metadata.is_dir())
    {
        return Err(Error::Path);
    }
    let mut git = command();
    git.current_dir(path);
    let root = run(
        runner,
        git.args(["rev-parse", "--is-inside-work-tree", "--show-prefix"]),
        |_| Error::NotRepository,
    )
    .await?;
    if root != b"true\n\n" {
        return Err(Error::NotRepository);
    }

    let configured = run(
        runner,
        command()
            .current_dir(path)
            .args(["config", "--get-all", "remote.origin.url"]),
        |_| Error::Origin,
    )
    .await?;
    matching_remote(&configured, repository)?;
    let resolved = run(
        runner,
        command()
            .current_dir(path)
            .args(["remote", "get-url", "--all", "origin"]),
        |_| Error::Origin,
    )
    .await?;
    matching_remote(&resolved, repository).map(str::to_owned)
}

async fn tracking_commit(runner: &CommandRunner, path: &Path, tracking: &str) -> Option<String> {
    let refs = run(
        runner,
        command().current_dir(path).args([
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            tracking,
        ]),
        |_| Error::Revision,
    )
    .await
    .ok()?;
    // Git also lists refs below the prefix, so match the full name.
    let prefix = format!("{tracking} ");
    refs.split(|&byte| byte == b'\n')
        .find_map(|line| line.strip_prefix(prefix.as_bytes()))
        .map(commit_id)
}

async fn worktree_dirty(runner: &CommandRunner, path: &Path) -> Result<bool, Error> {
    let output = run(
        runner,
        command().current_dir(path).args([
            "--no-optional-locks",
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=normal",
            "--ignore-submodules=none",
        ]),
        Error::Inspect,
    )
    .await?;
    Ok(!output.is_empty())
}

pub(super) fn tracking_ref(branch: &str) -> String {
    format!("refs/remotes/origin/{branch}")
}

pub(super) async fn current_branch(runner: &CommandRunner, path: &Path) -> Result<Vec<u8>, Error> {
    let head = run(
        runner,
        command()
            .current_dir(path)
            .args(["symbolic-ref", "--quiet", "HEAD"]),
        |_| Error::Revision,
    )
    .await?;
    head.strip_prefix(b"refs/heads/")
        .and_then(|branch| branch.strip_suffix(b"\n"))
        .map(<[u8]>::to_vec)
        .ok_or(Error::Revision)
}

pub(super) async fn revision(
    runner: &CommandRunner,
    path: &Path,
    reference: &str,
) -> Result<Vec<u8>, Error> {
    run(
        runner,
        command()
            .current_dir(path)
            .args(["rev-parse", "--verify", reference]),
        |_| Error::Revision,
    )
    .await
}

pub(super) fn commit_id(revision: &[u8]) -> String {
    String::from_utf8_lossy(revision.trim_ascii()).into_owned()
}

pub(super) async fn require_clean(runner: &CommandRunner, path: &Path) -> Result<(), Error> {
    if worktree_dirty(runner, path).await? {
        return Err(Error::LocalChanges);
    }
    Ok(())
}
