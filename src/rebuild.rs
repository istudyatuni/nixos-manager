use std::{
    ffi::{OsStr, OsString},
    os::unix::ffi::OsStrExt,
    path::Path,
};

use rustix::fs::{Access, access};
use tokio::{fs, process::Command, sync::mpsc};
use url::Url;

use crate::{
    command_runner::{CommandRunner, RunError},
    config::Config,
    git::{self, WorkingCopy},
};

pub const PKEXEC: &str = "/run/wrappers/bin/pkexec";
pub const NIXOS_REBUILD: &str = "/run/current-system/sw/bin/nixos-rebuild";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Test,
    Boot,
    Switch,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Test => "test",
            Self::Boot => "boot",
            Self::Switch => "switch",
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct Tools {
    pub pkexec: bool,
    pub nixos_rebuild: bool,
}

impl Tools {
    pub fn detect() -> Self {
        let executable =
            |path: &str| Path::new(path).is_file() && access(path, Access::EXEC_OK).is_ok();
        Self {
            pkexec: executable(PKEXEC),
            nixos_rebuild: executable(NIXOS_REBUILD),
        }
    }

    pub fn available(self) -> bool {
        self.pkexec && self.nixos_rebuild
    }
}

#[derive(Clone, Copy)]
pub enum Outcome {
    Success,
    Cancelled,
    AuthorizationFailed,
    Exited(i32),
    Interrupted,
    LaunchFailed,
    WaitFailed,
    InvalidRepository,
    Unsynced,
    MissingHostname,
    InvalidPath,
    MissingFlake,
}

#[derive(Clone)]
pub enum Event {
    Inspected(WorkingCopy),
    InterruptFailed,
    Finished(Outcome),
}

pub struct Run {
    pub mode: Mode,
    pub outcome: Option<Outcome>,
    pub interrupting: bool,
}

impl Run {
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            outcome: None,
            interrupting: false,
        }
    }

    pub fn is_running(&self) -> bool {
        self.outcome.is_none()
    }
}

pub async fn run<F: Future<Output = ()> + Send>(
    runner: &CommandRunner,
    config: &Config,
    mode: Mode,
    mut interrupt: mpsc::Receiver<()>,
    mut emit: impl FnMut(Event) -> F + Send,
) -> Outcome {
    let inspection = async {
        let status = git::inspect(runner, config).await?;
        let flake = if status.matches_revision {
            flake_reference(&config.path, &config.flake_directory).await
        } else {
            Err(Outcome::Unsynced)
        };
        Ok::<_, git::Error>((status, flake))
    };
    let result = tokio::select! {
        biased;
        Some(()) = interrupt.recv() => return Outcome::Interrupted,
        result = inspection => result,
    };
    let (status, flake) = match result {
        Ok(inspection) => inspection,
        Err(_) => {
            emit(Event::Inspected(WorkingCopy::default())).await;
            return Outcome::InvalidRepository;
        }
    };
    emit(Event::Inspected(status)).await;
    let mut selector = match flake {
        Ok(reference) => reference,
        Err(outcome) => return outcome,
    };

    let system = rustix::system::uname();
    let hostname = system.nodename().to_bytes();
    if hostname.is_empty() {
        return Outcome::MissingHostname;
    }
    selector.push("#");
    selector.push(OsStr::from_bytes(hostname));

    let mut command = Command::new(PKEXEC);
    command
        // pkexec validates an inherited SHELL against /etc/shells.
        .env_remove("SHELL")
        .args([
            "--disable-internal-agent",
            NIXOS_REBUILD,
            mode.as_str(),
            "--flake",
        ])
        .arg(selector);
    match runner
        .interruptible(command, interrupt, || emit(Event::InterruptFailed))
        .await
    {
        Ok(status) => match status.code() {
            Some(0) => Outcome::Success,
            Some(126) => Outcome::Cancelled,
            Some(127) => Outcome::AuthorizationFailed,
            Some(code) => Outcome::Exited(code),
            None => Outcome::Interrupted,
        },
        Err(RunError::Interrupted) => Outcome::Interrupted,
        Err(RunError::Launch) => Outcome::LaunchFailed,
        Err(RunError::Wait) => Outcome::WaitFailed,
    }
}

async fn flake_reference(repository: &Path, directory: &Path) -> Result<OsString, Outcome> {
    let root = fs::canonicalize(repository)
        .await
        .map_err(|_| Outcome::InvalidPath)?;
    let path = fs::canonicalize(root.join(directory))
        .await
        .map_err(|_| Outcome::MissingFlake)?;
    let relative = path.strip_prefix(&root).map_err(|_| Outcome::InvalidPath)?;
    let file = fs::canonicalize(path.join("flake.nix"))
        .await
        .map_err(|_| Outcome::MissingFlake)?;
    if !file.starts_with(&root) {
        return Err(Outcome::InvalidPath);
    }
    if !fs::metadata(file)
        .await
        .is_ok_and(|metadata| metadata.is_file())
    {
        return Err(Outcome::MissingFlake);
    }

    let path_bytes = path.as_os_str().as_bytes();
    if !path_bytes.contains(&b'#') && !path_bytes.contains(&b'?') {
        return Ok(path.into_os_string());
    }

    // For paths with flake delimiters, Nix expects the Git root and a separate dir parameter.
    let mut url = Url::from_file_path(&root).map_err(|_| Outcome::InvalidPath)?;
    if !relative.as_os_str().is_empty() {
        let encoded: String =
            url::form_urlencoded::byte_serialize(relative.as_os_str().as_bytes()).collect();
        let dir = encoded.replace('+', "%20");
        url.set_query(Some(&format!("dir={dir}")));
    }
    Ok(format!("git+{url}").into())
}
