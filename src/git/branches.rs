use std::{
    cmp::Reverse,
    collections::HashMap,
    ffi::OsStr,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    time::Duration,
};

use tokio::process::Command;

use crate::{command_runner::CommandRunner, config::valid_branch};

use super::{
    Error,
    command::{command, run},
    remote::{matching_remote, network_command},
    worktree::current_branch,
};

#[derive(Clone)]
pub struct Branch {
    name: String,
    committed_at: Option<i64>,
}

#[derive(Clone)]
pub struct RemoteRefs {
    pub branches: Option<Vec<Branch>>,
    pub default_branch: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteBranchesError {
    Network,
    TimedOut(Duration),
    AccessDenied,
    Authentication,
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

pub async fn read_branches(runner: &CommandRunner, path: &Path) -> Result<Vec<Branch>, Error> {
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
    reference: Option<&Path>,
) -> Result<RemoteRefs, RemoteBranchesError> {
    use RemoteBranchesError::Unavailable;
    let directory = tempfile::tempdir().map_err(|_| Unavailable)?;
    let mut git = ref_lookup_command(runner, directory.path(), repository, token).await?;
    // A treeless shallow clone keeps branch-tip commit dates out of the working copy.
    git.args([
        "clone",
        "--bare",
        "--depth=1",
        "--no-single-branch",
        "--filter=tree:0",
        "--no-tags",
        "--no-recurse-submodules",
        "--template=",
    ]);
    if let Some(worktree) = reference
        && let Some(reference) = alternate_for(runner, worktree).await
    {
        git.arg("--reference-if-able").arg(reference);
    }
    git.args(["--", repository]).arg(directory.path());
    run_ref_lookup(runner, &mut git, !token.is_empty()).await?;

    let branches = read_branches(runner, directory.path())
        .await
        .map_err(|_| Unavailable)?;
    let default_branch = current_branch(runner, directory.path())
        .await
        .ok()
        .and_then(|head| String::from_utf8(head).ok())
        .filter(|head| branches.iter().any(|branch| &branch.name == head));
    Ok(RemoteRefs {
        branches: Some(branches),
        default_branch,
    })
}

pub async fn remote_head(
    runner: &CommandRunner,
    repository: &str,
    token: &str,
) -> Result<RemoteRefs, RemoteBranchesError> {
    let directory = tempfile::tempdir().map_err(|_| RemoteBranchesError::Unavailable)?;
    let mut git = ref_lookup_command(runner, directory.path(), repository, token).await?;
    let output = run_ref_lookup(
        runner,
        git.args(["ls-remote", "--symref", "--", repository, "HEAD"]),
        !token.is_empty(),
    )
    .await?;
    Ok(RemoteRefs {
        branches: None,
        default_branch: parse_remote_head(&output),
    })
}

fn parse_remote_head(output: &[u8]) -> Option<String> {
    output.split(|&byte| byte == b'\n').find_map(|line| {
        let name = std::str::from_utf8(line)
            .ok()?
            .strip_prefix("ref: refs/heads/")?
            .strip_suffix("\tHEAD")?;
        valid_branch(name).then(|| name.to_owned())
    })
}

async fn ref_lookup_command(
    runner: &CommandRunner,
    directory: &Path,
    repository: &str,
    token: &str,
) -> Result<Command, RemoteBranchesError> {
    use RemoteBranchesError::Unavailable;
    let resolved = run(
        runner,
        command()
            .current_dir(directory)
            .args(["ls-remote", "--get-url", "--", repository]),
        Error::Fetch,
    )
    .await
    .map_err(|_| Unavailable)?;
    let remote = matching_remote(&resolved, repository).map_err(|_| Unavailable)?;
    let mut git = network_command(directory, repository, token, remote).map_err(|_| Unavailable)?;
    git.env("LC_ALL", "C")
        .args(["-c", "http.lowSpeedLimit=1", "-c", "http.lowSpeedTime=10"]);
    Ok(git)
}

async fn run_ref_lookup(
    runner: &CommandRunner,
    git: &mut Command,
    has_token: bool,
) -> Result<Vec<u8>, RemoteBranchesError> {
    let timeout = Duration::from_secs(15);
    let output = tokio::time::timeout(timeout, runner.output(git))
        .await
        .map_err(|_| RemoteBranchesError::TimedOut(timeout))?
        .map_err(|_| RemoteBranchesError::Unavailable)?;
    if !output.status.success() {
        return Err(ref_lookup_error(&output.stderr, has_token));
    }

    Ok(output.stdout)
}

fn ref_lookup_error(stderr: &[u8], has_token: bool) -> RemoteBranchesError {
    use RemoteBranchesError::{AccessDenied, Authentication, Network, Unavailable};

    if remote_transport_failed(stderr) {
        return Network;
    }
    let stderr = String::from_utf8_lossy(stderr);
    for line in stderr.lines() {
        let http_status = line
            .strip_prefix("fatal: unable to access '")
            .and_then(|detail| detail.split_once("': The requested URL returned error: "))
            .map(|(_, status)| status.trim());
        if line.starts_with("remote: Invalid username or token.")
            || line.starts_with("fatal: Authentication failed for '")
            || http_status == Some("401")
        {
            return if has_token {
                Authentication
            } else {
                AccessDenied
            };
        }
        if matches!(http_status, Some("403" | "404"))
            || line == "remote: Repository not found."
            || line.starts_with("fatal: repository '") && line.ends_with("' not found")
            || !has_token
                && line.starts_with("fatal: could not read Username for ")
                && line.ends_with(": terminal prompts disabled")
        {
            return AccessDenied;
        }
    }
    Unavailable
}

async fn alternate_for(runner: &CommandRunner, path: &Path) -> Option<PathBuf> {
    let git_dir = run(
        runner,
        command().current_dir(path).args([
            "rev-parse",
            "--path-format=absolute",
            "--git-common-dir",
        ]),
        |_| Error::NotRepository,
    )
    .await
    .ok()?;
    let git_dir = git_dir.strip_suffix(b"\n")?;
    // Git writes alternate paths one per line.
    (!git_dir.is_empty() && !git_dir.contains(&b'\n'))
        .then(|| PathBuf::from(OsStr::from_bytes(git_dir)))
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

#[cfg(test)]
mod tests {
    use super::{RemoteBranchesError, parse_remote_head, ref_lookup_error};

    #[test]
    fn reads_only_the_symbolic_head_of_the_remote() {
        assert_eq!(
            parse_remote_head(b"ref: refs/heads/release/main\tHEAD\n123456\tHEAD\n").as_deref(),
            Some("release/main")
        );
        assert_eq!(parse_remote_head(b"123456\tHEAD\n"), None);
        assert_eq!(
            parse_remote_head(b"ref: refs/heads/other\trefs/heads/HEAD\n"),
            None
        );
        assert_eq!(parse_remote_head(b""), None);
    }

    #[test]
    fn classifies_only_explicit_git_diagnostics_as_access_failures() {
        use RemoteBranchesError::{AccessDenied, Authentication, Network, Unavailable};

        let cases: &[(&[u8], bool, RemoteBranchesError)] = &[
            (b"remote: Repository not found.\n", true, AccessDenied),
            (
                b"fatal: could not read Username for 'https://github.com': terminal prompts disabled\n",
                false,
                AccessDenied,
            ),
            (
                b"fatal: unable to access 'https://github.com/owner/repo/': The requested URL returned error: 403\n",
                true,
                AccessDenied,
            ),
            (
                b"remote: Invalid username or token. Password authentication is not supported for Git operations.\n",
                true,
                Authentication,
            ),
            (
                b"fatal: Authentication failed for 'https://github.com/owner/repo/'\n",
                false,
                AccessDenied,
            ),
            (
                b"fatal: unable to access 'https://github.com/owner/repo/': Could not resolve host: github.com\n",
                false,
                Network,
            ),
            (b"fatal: could not create work tree dir\n", true, Unavailable),
        ];
        for &(stderr, has_token, expected) in cases {
            assert_eq!(ref_lookup_error(stderr, has_token), expected);
        }
    }
}
