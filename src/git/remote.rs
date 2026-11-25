use std::{
    env,
    io::{self, BufRead, Write},
    path::Path,
};

use tokio::process::Command;

use crate::config::{github_url, valid_token};

use super::{Error, command::command};

pub const CREDENTIAL_HELPER_FLAG: &str = "--git-credential-helper";
const HELPER_EXECUTABLE: &str = "NIXOS_MANAGER_CREDENTIAL_EXECUTABLE";
const HELPER_REPOSITORY: &str = "NIXOS_MANAGER_CREDENTIAL_REPOSITORY";
const HELPER_TOKEN: &str = "NIXOS_MANAGER_CREDENTIAL_TOKEN";

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
        && valid_token(&token)
    {
        writeln!(
            io::stdout().lock(),
            "username=x-access-token\npassword={token}\n"
        )?;
    }
    Ok(())
}

fn same_repository_path(first: &str, second: &str) -> bool {
    fn name(path: &str) -> &str {
        let path = path.strip_suffix('/').unwrap_or(path);
        path.strip_suffix(".git").unwrap_or(path)
    }
    name(first.strip_prefix('/').unwrap_or(first))
        .eq_ignore_ascii_case(name(second.strip_prefix('/').unwrap_or(second)))
}

pub(super) fn matching_remote<'a>(output: &'a [u8], repository: &str) -> Result<&'a str, Error> {
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

pub(super) fn network_command(
    path: &Path,
    repository: &str,
    token: &str,
    remote: &str,
) -> Result<Command, Error> {
    let mut git = command();
    git.current_dir(path)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ALLOW_PROTOCOL", "https")
        .env_remove("GIT_ASKPASS")
        .env_remove("SSH_ASKPASS")
        .env_remove("GIT_CURL_VERBOSE")
        .env_remove("GIT_SSL_NO_VERIFY")
        .env_remove(HELPER_EXECUTABLE)
        .env_remove(HELPER_REPOSITORY)
        .env_remove(HELPER_TOKEN)
        .args([
            "-c",
            "core.askPass=",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "credential.helper=",
        ])
        .arg("-c")
        .arg(format!("http.{remote}.followRedirects=false"))
        .arg("-c")
        .arg(format!("http.{remote}.sslVerify=true"));
    if !token.is_empty() {
        let executable = env::current_exe().map_err(|_| Error::Unavailable)?;
        git.env(HELPER_EXECUTABLE, executable)
            .env(HELPER_REPOSITORY, repository)
            .env(HELPER_TOKEN, token)
            .arg("-c")
            .arg(format!(
                "credential.helper=!exec \"${HELPER_EXECUTABLE}\" {CREDENTIAL_HELPER_FLAG}"
            ))
            .arg("-c")
            .arg(format!("credential.{remote}.useHttpPath=true"));
    }
    for (key, _) in env::vars_os() {
        if key.as_encoded_bytes().starts_with(b"GIT_TRACE") {
            git.env_remove(key);
        }
    }
    Ok(git)
}
