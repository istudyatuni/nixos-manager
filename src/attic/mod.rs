use std::{
    env,
    ffi::OsStr,
    fs, io,
    os::unix::fs::PermissionsExt,
    path::Path,
    time::{Duration, SystemTime},
};

use chrono::{DateTime, Datelike, SecondsFormat, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use tokio::{process::Command, time::timeout};
use url::Url;

use crate::{
    command_runner::{CommandRunner, OutputError},
    config::{self, CONFIG_DIRECTORY},
};

mod token;
pub use token::{Error as TokenError, Token};

#[cfg(test)]
pub(crate) mod tests;

const METADATA_FILE: &str = "attic.json";
const CLI: &str = "attic";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
pub const DEFAULT_SERVER_NAME: &str = "private";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Metadata {
    pub cache_name: String,
    pub cache_expiration: DateTime<Utc>,
    pub server_url: String,
    pub server_name: String,
}

#[derive(Clone, Copy, Debug)]
pub struct LoadError;

pub fn load(home: Option<&Path>) -> Result<Option<Metadata>, LoadError> {
    let directory = home.ok_or(LoadError)?.join(CONFIG_DIRECTORY);
    let metadata =
        config::read_json::<Metadata>(&directory.join(METADATA_FILE)).map_err(|_| LoadError)?;
    if let Some(metadata) = &metadata
        && (server_url(&metadata.server_url).is_none()
            || !valid_name(&metadata.server_name)
            || !valid_name(&metadata.cache_name)
            || !(0..=9999).contains(&metadata.cache_expiration.year()))
    {
        return Err(LoadError);
    }
    Ok(metadata)
}

pub fn forget(home: Option<&Path>) -> io::Result<()> {
    let home = home.ok_or_else(|| io::Error::other("missing home directory"))?;
    match fs::remove_file(home.join(CONFIG_DIRECTORY).join(METADATA_FILE)) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpirationWarning {
    Expired,
    Expiring,
}

pub fn now() -> DateTime<Utc> {
    SystemTime::now().into()
}

impl Metadata {
    pub fn expiration_rfc3339(&self) -> String {
        self.cache_expiration
            .to_rfc3339_opts(SecondsFormat::Secs, true)
    }

    pub fn warning(&self, now: DateTime<Utc>) -> Option<ExpirationWarning> {
        let remaining = self.cache_expiration - now;
        if remaining <= TimeDelta::zero() {
            Some(ExpirationWarning::Expired)
        } else if remaining < TimeDelta::hours(24) {
            Some(ExpirationWarning::Expiring)
        } else {
            None
        }
    }
}

pub fn available(path: Option<&OsStr>) -> bool {
    path.is_some_and(|path| {
        env::split_paths(path).any(|directory| directory.join(CLI).symlink_metadata().is_ok())
    })
}

pub fn valid_name(name: &str) -> bool {
    name.len() <= 50
        && name
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_+".contains(&byte))
}

pub fn server_url(input: &str) -> Option<Url> {
    let input = input.trim();
    let (scheme, rest) = input.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    if !["http", "https"]
        .iter()
        .any(|allowed| scheme.eq_ignore_ascii_case(allowed))
        || authority.is_empty()
        || authority.contains('@')
        || input
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control() || byte == b'\\')
    {
        return None;
    }
    let url = Url::parse(input).ok()?;
    (url.has_host() && url.query().is_none() && url.fragment().is_none()).then_some(url)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationError {
    Url,
    ServerName,
    Token(TokenError),
    CacheName,
}

pub struct Connection {
    metadata: Metadata,
    secret: String,
}

impl Connection {
    pub fn new(
        url: &str,
        server_name: &str,
        token: &str,
        cache_name: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<Self, ValidationError> {
        let url = server_url(url).ok_or(ValidationError::Url)?;
        let server_name = server_name.trim();
        if !valid_name(server_name) {
            return Err(ValidationError::ServerName);
        }
        let secret = token.trim();
        let token = Token::parse(secret, now).map_err(ValidationError::Token)?;
        let Some(cache_name) =
            cache_name.filter(|&name| token.cache_names.iter().any(|cache| cache == name))
        else {
            return Err(ValidationError::CacheName);
        };
        Ok(Self {
            metadata: Metadata {
                cache_name: cache_name.to_owned(),
                cache_expiration: token.expiration,
                server_url: url.into(),
                server_name: server_name.to_owned(),
            },
            secret: secret.to_owned(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    Launch,
    Timeout,
    AccessDenied,
    ConfigWrite,
    MetadataWrite,
    VerificationSetup,
    Cli,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Prepare,
    VerifyLogin,
    VerifyCache,
    ApplyLogin,
    ApplyCache,
    Save,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Failure {
    pub stage: Stage,
    pub kind: FailureKind,
}

impl Failure {
    pub fn may_have_applied(self) -> bool {
        matches!(self.stage, Stage::ApplyCache | Stage::Save)
            || (self.stage == Stage::ApplyLogin && self.kind != FailureKind::Launch)
    }
}

pub async fn connect(
    home: Option<&Path>,
    runner: CommandRunner,
    connection: Connection,
) -> Result<Metadata, Failure> {
    connect_with(home, runner, connection, COMMAND_TIMEOUT, |_| {}).await
}

async fn connect_with(
    home: Option<&Path>,
    runner: CommandRunner,
    Connection { metadata, secret }: Connection,
    command_timeout: Duration,
    configure_command: impl Fn(&mut Command),
) -> Result<Metadata, Failure> {
    let directory = home.map(|home| home.join(CONFIG_DIRECTORY));
    let prepare_error = Failure {
        stage: Stage::Prepare,
        kind: FailureKind::MetadataWrite,
    };
    let directory = directory.ok_or(prepare_error)?;
    let temp_metadata = {
        let directory = directory.clone();
        let metadata = metadata.clone();
        tokio::task::spawn_blocking(move || config::prepare_json(&directory, &metadata))
            .await
            .map_err(|_| prepare_error)?
            .map_err(|_| prepare_error)?
    };
    let runner = runner
        .with_secret(secret.clone())
        .with_output_redactor(redact_config_diagnostics);
    let reference = format!("{}:{}", metadata.server_name, metadata.cache_name);
    let command = || {
        let mut command = Command::new(CLI);
        configure_command(&mut command);
        command
    };
    let login = || {
        let mut command = command();
        command.args([
            "login",
            "--set-default",
            &metadata.server_name,
            &metadata.server_url,
            &secret,
        ]);
        command
    };

    {
        let isolated = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .map_err(|_| Failure {
                stage: Stage::Prepare,
                kind: FailureKind::VerificationSetup,
            })?;
        run(
            &runner,
            login().env("XDG_CONFIG_HOME", isolated.path()),
            Stage::VerifyLogin,
            command_timeout,
        )
        .await?;
        run(
            &runner,
            command()
                .args(["cache", "info", &reference])
                .env("XDG_CONFIG_HOME", isolated.path()),
            Stage::VerifyCache,
            command_timeout,
        )
        .await?;
    }

    run(&runner, &mut login(), Stage::ApplyLogin, command_timeout).await?;
    run(
        &runner,
        command().args(["use", &reference]),
        Stage::ApplyCache,
        command_timeout,
    )
    .await?;

    let save_error = Failure {
        stage: Stage::Save,
        kind: FailureKind::MetadataWrite,
    };
    tokio::task::spawn_blocking(move || temp_metadata.persist(directory.join(METADATA_FILE)))
        .await
        .map_err(|_| save_error)?
        .map_err(|_| save_error)?;
    Ok(metadata)
}

async fn run(
    runner: &CommandRunner,
    command: &mut Command,
    stage: Stage,
    duration: Duration,
) -> Result<(), Failure> {
    let failure = |kind| Failure { stage, kind };
    let login = matches!(stage, Stage::VerifyLogin | Stage::ApplyLogin);
    if login {
        // Attic can report failed config writes in tracing output while exiting successfully.
        command.env("RUST_LOG", "warn");
    }
    let output = timeout(duration, runner.output_with_error_kind(command))
        .await
        .map_err(|_| failure(FailureKind::Timeout))?
        .map_err(|error| {
            failure(match error {
                OutputError::Launch(_) => FailureKind::Launch,
                OutputError::Wait(_) => FailureKind::Cli,
            })
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    let diagnostic = |message: &str| stdout.contains(message) || stderr.contains(message);
    if login
        && (diagnostic("could not save modified configuration")
            || diagnostic("could not get config path"))
    {
        return Err(failure(FailureKind::ConfigWrite));
    }

    if output.status.success() {
        Ok(())
    } else if diagnostic("error: unauthorized") || diagnostic("error: forbidden") {
        Err(failure(FailureKind::AccessDenied))
    } else if stage != Stage::VerifyCache
        && (diagnostic("permission denied")
            || diagnostic("read-only file system")
            || diagnostic("no space left on device")
            || diagnostic("failed to write"))
    {
        Err(failure(FailureKind::ConfigWrite))
    } else {
        Err(failure(FailureKind::Cli))
    }
}

fn redact_config_diagnostics(line: &mut String) {
    // A parser may echo a source line or a credential from an existing config.
    if let Some((number, _)) = line.split_once(" |")
        && number.trim().parse::<usize>().is_ok()
    {
        line.truncate(number.len() + 2);
        line.push_str(" ***");
    } else if let Some((prefix, rest)) = line.split_once("Unknown token ")
        && let Some(value) = rest.strip_suffix(" outside a machine block")
    {
        let start = prefix.len() + "Unknown token ".len();
        line.replace_range(start..start + value.len(), "***");
    } else if let Some((prefix, rest)) = line.split_once("Line \"")
        && let Some(value) = rest.strip_suffix("\" isn't valid")
    {
        let start = prefix.len() + "Line \"".len();
        line.replace_range(start..start + value.len(), "***");
    }
}
