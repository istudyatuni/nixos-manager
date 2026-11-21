use std::{
    env, fs,
    io::{self, BufRead, Write},
    path::Path,
    process::{Command, Stdio},
};

use tempfile::Builder;

use crate::config::{Config, github_url};

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

pub fn available() -> bool {
    command()
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

pub fn has_local_changes(path: &Path, repository: &str) -> Result<bool, Error> {
    check_worktree(path, repository)?;
    worktree_dirty(path)
}

pub fn download(config: &Config) -> Result<(), Error> {
    let tracking_ref = format!("refs/remotes/origin/{}", config.branch);

    match fs::symlink_metadata(&config.path) {
        Ok(_) => synchronize(config, &tracking_ref),
        Err(error) if error.kind() == io::ErrorKind::NotFound => clone(config, &tracking_ref),
        Err(_) => Err(Error::Path),
    }
}

fn clone(config: &Config, tracking_ref: &str) -> Result<(), Error> {
    let parent = config.path.parent().ok_or(Error::Path)?;
    fs::create_dir_all(parent).map_err(|_| Error::Path)?;
    let staging = Builder::new()
        .prefix(".nixos-manager-")
        .tempdir_in(parent)
        .map_err(|_| Error::Path)?;

    let resolved = run(
        command().current_dir(staging.path()).args([
            "ls-remote",
            "--get-url",
            "--",
            &config.repository,
        ]),
        Error::Clone,
        &config.token,
    )?;
    let remote = matching_remote(&resolved, &config.repository)?;
    let mut git = network_command(staging.path(), config, remote)?;
    git.args([
        "clone",
        "--no-checkout",
        "--single-branch",
        "--no-tags",
        "--origin",
        "origin",
        "--branch",
        &config.branch,
        "--",
        &config.repository,
    ])
    .arg(staging.path());
    run(&mut git, Error::Clone, &config.token)?;

    check_worktree(staging.path(), &config.repository)?;
    checkout(staging.path(), config, tracking_ref)?;

    // Publish the clone only after checkout succeeds. Renaming a directory cannot
    // overwrite another clone because that destination contains .git.
    match fs::symlink_metadata(&config.path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::rename(staging.path(), &config.path).map_err(|_| Error::Path)
        }
        _ => Err(Error::Path),
    }
}

fn synchronize(config: &Config, tracking_ref: &str) -> Result<(), Error> {
    let remote = check_worktree(&config.path, &config.repository)?;
    require_clean(&config.path)?;

    let refspec = format!("refs/heads/{}:{tracking_ref}", config.branch);
    let mut git = network_command(&config.path, config, &remote)?;
    run(
        git.args([
            "fetch",
            "--force",
            "--no-tags",
            "--no-prune",
            "--no-recurse-submodules",
            "--no-auto-maintenance",
            "--refmap=",
            "origin",
            &refspec,
        ]),
        Error::Fetch,
        &config.token,
    )?;

    require_clean(&config.path)?;
    checkout(&config.path, config, tracking_ref)
}

fn checkout(path: &Path, config: &Config, tracking_ref: &str) -> Result<(), Error> {
    let expected = revision(path, tracking_ref)?;
    run(
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
        &config.token,
    )?;

    let branch = run(
        command()
            .current_dir(path)
            .args(["symbolic-ref", "--quiet", "HEAD"]),
        |_| Error::Revision,
        &config.token,
    )?;
    if branch.strip_suffix(b"\n") != Some(format!("refs/heads/{}", config.branch).as_bytes())
        || revision(path, "HEAD")? != expected
        || revision(path, tracking_ref)? != expected
    {
        return Err(Error::Revision);
    }
    Ok(())
}

fn revision(path: &Path, reference: &str) -> Result<Vec<u8>, Error> {
    run(
        command()
            .current_dir(path)
            .args(["rev-parse", "--verify", reference]),
        |_| Error::Revision,
        "",
    )
}

fn check_worktree(path: &Path, repository: &str) -> Result<String, Error> {
    if !path.is_dir() {
        return Err(Error::Path);
    }
    let mut git = command();
    git.current_dir(path);
    let root = run(
        git.args(["rev-parse", "--is-inside-work-tree", "--show-prefix"]),
        |_| Error::NotRepository,
        "",
    )?;
    if root != b"true\n\n" {
        return Err(Error::NotRepository);
    }

    let configured = run(
        command()
            .current_dir(path)
            .args(["config", "--get-all", "remote.origin.url"]),
        |_| Error::Origin,
        "",
    )?;
    matching_remote(&configured, repository)?;
    let resolved = run(
        command()
            .current_dir(path)
            .args(["remote", "get-url", "--all", "origin"]),
        |_| Error::Origin,
        "",
    )?;
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

fn require_clean(path: &Path) -> Result<(), Error> {
    if worktree_dirty(path)? {
        return Err(Error::LocalChanges);
    }
    Ok(())
}

fn worktree_dirty(path: &Path) -> Result<bool, Error> {
    let output = run(
        command().current_dir(path).args([
            "--no-optional-locks",
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=normal",
            "--ignore-submodules=none",
        ]),
        Error::Inspect,
        "",
    )?;
    Ok(!output.is_empty())
}

fn command() -> Command {
    let mut git = Command::new("git");
    git.stdin(Stdio::null());
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

fn network_command(path: &Path, config: &Config, remote: &str) -> Result<Command, Error> {
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
        .env(HELPER_REPOSITORY, &config.repository)
        .env(HELPER_TOKEN, &config.token)
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

fn run(
    command: &mut Command,
    failure: impl FnOnce(String) -> Error,
    token: &str,
) -> Result<Vec<u8>, Error> {
    match command.output() {
        Ok(output) if output.status.success() => Ok(output.stdout),
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let detail = if token.is_empty() {
                stderr.trim().to_owned()
            } else {
                stderr.trim().replace(token, "***")
            };
            Err(failure(detail))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(Error::Unavailable),
        Err(error) => Err(failure(error.to_string())),
    }
}
