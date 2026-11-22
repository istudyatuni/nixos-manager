use std::{
    cmp::Reverse,
    collections::HashMap,
    env,
    io::{self, BufRead, Write},
    path::Path,
    time::Duration,
};

use tempfile::{Builder, TempDir};
use tokio::{fs, process::Command};

use crate::{
    command_runner::CommandRunner,
    config::{Config, github_url, valid_branch},
};

pub const CREDENTIAL_HELPER_FLAG: &str = "--git-credential-helper";
const HELPER_EXECUTABLE: &str = "NIXOS_MANAGER_CREDENTIAL_EXECUTABLE";
const HELPER_REPOSITORY: &str = "NIXOS_MANAGER_CREDENTIAL_REPOSITORY";
const HELPER_TOKEN: &str = "NIXOS_MANAGER_CREDENTIAL_TOKEN";

#[derive(Clone, Debug)]
pub enum Error {
    Unavailable,
    Path,
    NotRepository,
    Origin,
    LocalChanges,
    Inspect(String),
    Clone(String),
    Fetch(String),
    Checkout(String),
    Revision,
}

pub async fn available(runner: &CommandRunner) -> bool {
    runner
        .output(command().arg("--version"))
        .await
        .is_ok_and(|output| output.status.success())
}

#[derive(Clone)]
pub struct Branch {
    name: String,
    committed_at: Option<i64>,
}

#[derive(Clone)]
pub struct RemoteBranches {
    pub branches: Vec<Branch>,
    pub default_branch: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub enum RemoteBranchesError {
    Network,
    Unavailable,
}

pub fn branch_names(local: &[Branch], remote: &[Branch]) -> Vec<String> {
    let mut branches = HashMap::with_capacity(local.len() + remote.len());
    for branch in local.iter().chain(remote) {
        branches
            .entry(branch.name.as_str())
            .and_modify(|date| *date = branch.committed_at.max(*date))
            .or_insert(branch.committed_at);
    }
    let mut branches: Vec<_> = branches.into_iter().collect();
    branches.sort_unstable_by_key(|&(name, date)| (Reverse(date), name));
    branches
        .into_iter()
        .map(|(name, _)| name.to_owned())
        .collect()
}

pub async fn local_branches(runner: &CommandRunner, path: &Path, repository: &str) -> Vec<Branch> {
    if check_worktree(runner, path, repository).await.is_err() {
        return Vec::new();
    }
    read_branches(runner, path).await.unwrap_or_default()
}

async fn read_branches(runner: &CommandRunner, path: &Path) -> Result<Vec<Branch>, Error> {
    let output = run(
        runner,
        command()
            .current_dir(path)
            .env("GIT_NO_LAZY_FETCH", "1")
            .args([
                "for-each-ref",
                "--format=%(refname:strip=2)%09%(committerdate:unix)%09%(symref)",
                "refs/heads",
            ]),
        Error::Inspect,
    )
    .await?;
    let branches = output
        .split(|&byte| byte == b'\n')
        .filter_map(|line| {
            let (name, date) = std::str::from_utf8(line)
                .ok()?
                .strip_suffix('\t')?
                .split_once('\t')?;
            valid_branch(name).then(|| Branch {
                name: name.to_owned(),
                committed_at: date.parse().ok(),
            })
        })
        .collect();
    Ok(branches)
}

pub async fn remote_branches(
    runner: &CommandRunner,
    repository: &str,
    token: &str,
) -> Result<RemoteBranches, RemoteBranchesError> {
    use RemoteBranchesError::{Network, Unavailable};

    let directory = tempfile::tempdir().map_err(|_| Unavailable)?;
    let resolved = run(
        runner,
        command()
            .current_dir(directory.path())
            .args(["ls-remote", "--get-url", "--", repository]),
        Error::Fetch,
    )
    .await
    .map_err(|_| Unavailable)?;
    let remote = matching_remote(&resolved, repository).map_err(|_| Unavailable)?;
    let mut git =
        network_command(directory.path(), repository, token, remote).map_err(|_| Unavailable)?;
    // A treeless shallow clone keeps branch-tip commit dates out of the working copy.
    git.env("LC_ALL", "C")
        .args([
            "-c",
            "http.lowSpeedLimit=1",
            "-c",
            "http.lowSpeedTime=10",
            "clone",
            "--bare",
            "--depth=1",
            "--no-single-branch",
            "--filter=tree:0",
            "--no-tags",
            "--no-recurse-submodules",
            "--template=",
            "--",
            repository,
        ])
        .arg(directory.path());
    let output = tokio::time::timeout(Duration::from_secs(15), runner.output(&mut git))
        .await
        .map_err(|_| Network)?
        .map_err(|_| Unavailable)?;
    if !output.status.success() {
        return Err(if remote_transport_failed(&output.stderr) {
            Network
        } else {
            Unavailable
        });
    }

    let branches = read_branches(runner, directory.path())
        .await
        .map_err(|_| Unavailable)?;
    let default_branch = current_branch(runner, directory.path())
        .await
        .ok()
        .and_then(|head| String::from_utf8(head).ok())
        .filter(|head| branches.iter().any(|branch| &branch.name == head));
    Ok(RemoteBranches {
        branches,
        default_branch,
    })
}

fn remote_transport_failed(stderr: &[u8]) -> bool {
    String::from_utf8_lossy(stderr).lines().any(|line| {
        // Git reports libcurl codes for failed RPCs and libcurl messages for ref discovery.
        if let Some(rpc) = line.strip_prefix("error: RPC failed; curl ") {
            let code = rpc
                .split_whitespace()
                .next()
                .and_then(|code| code.parse::<u8>().ok());
            return matches!(
                code,
                Some(5 | 6 | 7 | 16 | 18 | 28 | 35 | 52 | 55 | 56 | 60 | 92 | 95 | 96 | 97)
            );
        }
        let Some((_, detail)) = line
            .strip_prefix("fatal: unable to access '")
            .and_then(|line| line.split_once("': "))
        else {
            return false;
        };
        let detail = detail.to_ascii_lowercase();
        [
            "could not resolve",
            "couldn't resolve",
            "failed to connect",
            "couldn't connect",
            "timed out",
            "timeout",
            "operation too slow",
            "connection refused",
            "connection reset",
            "network is unreachable",
            "proxy connect",
            "connect tunnel",
            "send failure",
            "recv failure",
            "failure when sending data",
            "failure when receiving data",
            "empty reply",
            "server returned nothing",
            "ssl",
            "tls",
            "certificate verification",
            "http/2",
            "http/3",
        ]
        .iter()
        .any(|message| detail.contains(message))
    })
}

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

fn tracking_ref(branch: &str) -> String {
    format!("refs/remotes/origin/{branch}")
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

async fn current_branch(runner: &CommandRunner, path: &Path) -> Result<Vec<u8>, Error> {
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

async fn revision(runner: &CommandRunner, path: &Path, reference: &str) -> Result<Vec<u8>, Error> {
    run(
        runner,
        command()
            .current_dir(path)
            .args(["rev-parse", "--verify", reference]),
        |_| Error::Revision,
    )
    .await
}

fn commit_id(revision: &[u8]) -> String {
    String::from_utf8_lossy(revision.trim_ascii()).into_owned()
}

async fn check_worktree(
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

fn matching_remote<'a>(output: &'a [u8], repository: &str) -> Result<&'a str, Error> {
    let remote = std::str::from_utf8(output)
        .ok()
        .and_then(|output| output.strip_suffix('\n'))
        .filter(|remote| !remote.chars().any(char::is_whitespace))
        .ok_or(Error::Origin)?;
    let url = github_url(remote).ok_or(Error::Origin)?;
    let expected = github_url(repository).ok_or(Error::Origin)?;
    // Compare the literal URL path too. The URL parser resolves dot segments.
    let raw_path = remote
        .split_once("://")
        .and_then(|(_, rest)| rest.find('/').map(|start| &rest[start..]))
        .ok_or(Error::Origin)?;
    if url.query().is_some()
        || url.fragment().is_some()
        || !same_repository_path(url.path(), expected.path())
        || !same_repository_path(raw_path, expected.path())
    {
        return Err(Error::Origin);
    }
    Ok(remote)
}

fn same_repository_path(first: &str, second: &str) -> bool {
    fn name(path: &str) -> &str {
        let path = path.strip_suffix('/').unwrap_or(path);
        path.strip_suffix(".git").unwrap_or(path)
    }
    name(first.strip_prefix('/').unwrap_or(first))
        .eq_ignore_ascii_case(name(second.strip_prefix('/').unwrap_or(second)))
}

async fn require_clean(runner: &CommandRunner, path: &Path) -> Result<(), Error> {
    if worktree_dirty(runner, path).await? {
        return Err(Error::LocalChanges);
    }
    Ok(())
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

fn command() -> Command {
    let mut git = Command::new("git");
    for key in [
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_COMMON_DIR",
        "GIT_CONFIG",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
        "GIT_DIR",
        "GIT_GRAFT_FILE",
        "GIT_IMPLICIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_PREFIX",
        "GIT_REPLACE_REF_BASE",
        "GIT_SHALLOW_FILE",
        "GIT_WORK_TREE",
    ] {
        git.env_remove(key);
    }
    git
}

fn network_command(
    path: &Path,
    repository: &str,
    token: &str,
    remote: &str,
) -> Result<Command, Error> {
    let executable = env::current_exe().map_err(|_| Error::Unavailable)?;
    let mut git = command();
    git.current_dir(path)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ALLOW_PROTOCOL", "https")
        .env_remove("GIT_ASKPASS")
        .env_remove("SSH_ASKPASS")
        .env_remove("GIT_CURL_VERBOSE")
        .env_remove("GIT_SSL_NO_VERIFY")
        .env(HELPER_EXECUTABLE, executable)
        .env(HELPER_REPOSITORY, repository)
        .env(HELPER_TOKEN, token)
        .args([
            "-c",
            "core.askPass=",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "credential.helper=",
        ])
        .arg("-c")
        .arg(format!(
            "credential.helper=!exec \"${HELPER_EXECUTABLE}\" {CREDENTIAL_HELPER_FLAG}"
        ))
        .arg("-c")
        .arg(format!("credential.{remote}.useHttpPath=true"))
        .arg("-c")
        .arg(format!("http.{remote}.followRedirects=false"))
        .arg("-c")
        .arg(format!("http.{remote}.sslVerify=true"));
    for (key, _) in env::vars_os() {
        if key.as_encoded_bytes().starts_with(b"GIT_TRACE") {
            git.env_remove(key);
        }
    }
    Ok(git)
}

pub fn write_credential() -> io::Result<()> {
    let (Ok(repository), Ok(token)) = (env::var(HELPER_REPOSITORY), env::var(HELPER_TOKEN)) else {
        return Ok(());
    };
    let Some(repository) = github_url(&repository) else {
        return Ok(());
    };
    let (mut protocol, mut host, mut path) = (None, None, None);
    for line in io::stdin().lock().lines() {
        let line = line?;
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once('=') {
            match key {
                "protocol" => protocol = Some(value.to_owned()),
                "host" => host = Some(value.to_owned()),
                "path" => path = Some(value.to_owned()),
                _ => {}
            }
        }
    }
    if protocol.as_deref() == Some("https")
        && host.is_some_and(|host| {
            host.eq_ignore_ascii_case("github.com") || host.eq_ignore_ascii_case("github.com:443")
        })
        && path.is_some_and(|path| same_repository_path(&path, repository.path()))
        && !token.is_empty()
        && !token
            .chars()
            .any(|character| character.is_whitespace() || character == '\0')
    {
        writeln!(
            io::stdout().lock(),
            "username=x-access-token\npassword={token}\n"
        )?;
    }
    Ok(())
}

async fn run(
    runner: &CommandRunner,
    command: &mut Command,
    failure: impl FnOnce(String) -> Error,
) -> Result<Vec<u8>, Error> {
    match runner.output(command).await {
        Ok(output) if output.status.success() => Ok(output.stdout),
        Ok(output) => Err(failure(
            runner.redact(String::from_utf8_lossy(&output.stderr).trim()),
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(Error::Unavailable),
        Err(error) => Err(failure(runner.redact(&error.to_string()))),
    }
}
