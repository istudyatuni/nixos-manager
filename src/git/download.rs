use std::{io, path::Path};

use tempfile::{Builder, TempDir};
use tokio::fs;

use crate::{command_runner::CommandRunner, config::Config};

use super::{
    Error,
    command::{command, run},
    remote::{matching_remote, network_command},
    worktree::{
        WorkingCopy, check_worktree, commit_id, current_branch, require_clean, revision,
        tracking_ref,
    },
};

pub async fn download(runner: &CommandRunner, config: &Config) -> Result<WorkingCopy, Error> {
    let tracking_ref = tracking_ref(&config.branch);

    let head = match fs::symlink_metadata(&config.path).await {
        Ok(_) => synchronize(runner, config, &tracking_ref).await,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            clone(runner, config, &tracking_ref).await
        }
        Err(_) => Err(Error::Path),
    }?;
    let commit = commit_id(&head);
    Ok(WorkingCopy {
        branch: Some(config.branch.clone()),
        commit: Some(commit.clone()),
        selected_commit: Some(commit),
        has_local_changes: false,
        matches_revision: true,
    })
}

async fn clone(
    runner: &CommandRunner,
    config: &Config,
    tracking_ref: &str,
) -> Result<Vec<u8>, Error> {
    let parent = config.path.parent().ok_or(Error::Path)?.to_owned();
    fs::create_dir_all(&parent).await.map_err(|_| Error::Path)?;
    let staging = tokio::task::spawn_blocking(move || {
        Builder::new().prefix(".nixos-manager-").tempdir_in(parent)
    })
    .await
    .map_err(|_| Error::Path)?
    .map_err(|_| Error::Path)?;
    let result = clone_in(runner, config, tracking_ref, &staging).await;
    let _ = tokio::task::spawn_blocking(move || drop(staging)).await;
    result
}

async fn clone_in(
    runner: &CommandRunner,
    config: &Config,
    tracking_ref: &str,
    staging: &TempDir,
) -> Result<Vec<u8>, Error> {
    let resolved = run(
        runner,
        command().current_dir(staging.path()).args([
            "ls-remote",
            "--get-url",
            "--",
            &config.repository,
        ]),
        Error::Clone,
    )
    .await?;
    let remote = matching_remote(&resolved, &config.repository)?;
    let mut git = network_command(staging.path(), &config.repository, &config.token, remote)?;
    git.args([
        "clone",
        "--progress",
        "--no-checkout",
        "--no-single-branch",
        "--no-tags",
        "--origin",
        "origin",
        "--branch",
        &config.branch,
        "--",
        &config.repository,
    ])
    .arg(staging.path());
    run(runner, &mut git, Error::Clone).await?;

    check_worktree(runner, staging.path(), &config.repository).await?;
    let head = checkout(runner, staging.path(), config, tracking_ref).await?;

    // Publish the clone only after checkout succeeds. Renaming a directory cannot
    // overwrite another clone because that destination contains .git.
    match fs::symlink_metadata(&config.path).await {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::rename(staging.path(), &config.path)
                .await
                .map_err(|_| Error::Path)?;
            Ok(head)
        }
        _ => Err(Error::Path),
    }
}

async fn synchronize(
    runner: &CommandRunner,
    config: &Config,
    tracking_ref: &str,
) -> Result<Vec<u8>, Error> {
    let remote = check_worktree(runner, &config.path, &config.repository).await?;
    require_clean(runner, &config.path).await?;

    let mut git = network_command(&config.path, &config.repository, &config.token, &remote)?;
    run(
        runner,
        git.args([
            "fetch",
            "--progress",
            "--force",
            "--no-tags",
            "--prune",
            "--no-prune-tags",
            "--no-recurse-submodules",
            "--no-auto-maintenance",
            "--refmap=",
            "origin",
            "refs/heads/*:refs/remotes/origin/*",
        ]),
        Error::Fetch,
    )
    .await?;

    require_clean(runner, &config.path).await?;
    checkout(runner, &config.path, config, tracking_ref).await
}

async fn checkout(
    runner: &CommandRunner,
    path: &Path,
    config: &Config,
    tracking_ref: &str,
) -> Result<Vec<u8>, Error> {
    let expected = revision(runner, path, tracking_ref).await?;
    run(
        runner,
        command().current_dir(path).args([
            "checkout",
            "--no-overwrite-ignore",
            "--no-recurse-submodules",
            "-B",
            &config.branch,
            tracking_ref,
            "--",
        ]),
        Error::Checkout,
    )
    .await?;

    if current_branch(runner, path).await? != config.branch.as_bytes()
        || revision(runner, path, "HEAD").await? != expected
        || revision(runner, path, tracking_ref).await? != expected
    {
        return Err(Error::Revision);
    }
    Ok(expected)
}
