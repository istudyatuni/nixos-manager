use std::{
    ffi::{OsStr, OsString},
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::Arc,
};

use rustix::fs::{Access, access};
use tempfile::TempDir;
use tokio::{fs, process::Command, sync::mpsc};
use url::Url;

use crate::{
    command_runner::{CommandRunner, RunError},
    config::Config,
    git::{self, WorkingCopy},
};

const PKEXEC: &str = "/run/wrappers/bin/pkexec";
const NIX: &str = "/run/current-system/sw/bin/nix";
const NIX_ENV: &str = "/run/current-system/sw/bin/nix-env";
const SYSTEM_PROFILE: &str = "/nix/var/nix/profiles/system";
const STORE: &str = "/nix/store";
const OUT_LINK: &str = "result";
const SWITCH: &str = "bin/switch-to-configuration";

#[derive(Clone, Copy)]
pub struct Steps {
    pub boot: bool,
    pub activate: bool,
}

impl Default for Steps {
    fn default() -> Self {
        Self {
            boot: false,
            activate: true,
        }
    }
}

impl Steps {
    pub fn applies(self) -> bool {
        self.boot || self.activate
    }
}

#[derive(Clone, Copy, Default)]
pub struct Tools {
    nix: bool,
    pkexec: bool,
    nix_env: bool,
}

impl Tools {
    pub fn detect() -> Self {
        Self {
            nix: executable(Path::new(NIX)),
            pkexec: executable(Path::new(PKEXEC)),
            nix_env: executable(Path::new(NIX_ENV)),
        }
    }

    pub fn missing(self, needs_build: bool, steps: Steps) -> impl Iterator<Item = &'static str> {
        [
            (needs_build && !self.nix, NIX),
            (steps.applies() && !self.pkexec, PKEXEC),
            (steps.boot && !self.nix_env, NIX_ENV),
        ]
        .into_iter()
        .filter_map(|(missing, program)| missing.then_some(program))
    }
}

fn executable(path: &Path) -> bool {
    path.is_file() && access(path, Access::EXEC_OK).is_ok()
}

pub struct Build {
    head: Option<String>,
    directory: TempDir,
    path: PathBuf,
}

impl Build {
    pub fn matches(&self, status: &WorkingCopy) -> bool {
        status
            .clean_head()
            .is_some_and(|head| self.head.as_deref() == Some(head))
    }

    async fn result_path(directory: &TempDir) -> Option<PathBuf> {
        let path = fs::canonicalize(directory.path().join(OUT_LINK))
            .await
            .ok()?;
        if path.parent() != Some(Path::new(STORE))
            || !fs::metadata(path.join("nixos-version"))
                .await
                .is_ok_and(|metadata| metadata.is_file())
        {
            return None;
        }
        let script = fs::canonicalize(path.join(SWITCH)).await.ok()?;
        (script.starts_with(STORE) && executable(&script)).then_some(path)
    }
}

#[derive(Clone, Copy)]
pub enum Step {
    Inspect,
    Build,
    Register,
    Boot,
    Activate,
    BootAndActivate,
}

#[derive(Clone, Copy)]
pub enum Error {
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
    BuildDirectory,
    BuildResult,
    BuildOutdated,
}

#[derive(Clone)]
pub enum Event {
    Inspected(WorkingCopy),
    Started(Step),
    Built(Arc<Build>),
    InterruptFailed,
    Finished(Result<(), Error>),
}

pub struct Run {
    pub steps: Steps,
    pub step: Step,
    pub outcome: Option<Result<(), Error>>,
    pub interrupting: bool,
}

impl Run {
    pub fn new(steps: Steps) -> Self {
        Self {
            steps,
            step: Step::Inspect,
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
    steps: Steps,
    cached: Option<Arc<Build>>,
    mut interrupt: mpsc::Receiver<()>,
    mut emit: impl FnMut(Event) -> F + Send,
) -> Result<(), Error> {
    let inspection = tokio::select! {
        biased;
        Some(()) = interrupt.recv() => return Err(Error::Interrupted),
        result = git::inspect(runner, config) => result,
    };
    let status = match inspection {
        Ok(status) => status,
        Err(_) => {
            emit(Event::Inspected(WorkingCopy::default())).await;
            return Err(Error::InvalidRepository);
        }
    };
    emit(Event::Inspected(status.clone())).await;
    if !status.matches_revision {
        return Err(Error::Unsynced);
    }

    let build = if let Some(build) = cached {
        if !build.matches(&status) {
            return Err(Error::BuildOutdated);
        }
        if Build::result_path(&build.directory).await.as_ref() != Some(&build.path) {
            return Err(Error::BuildResult);
        }
        build
    } else {
        emit(Event::Started(Step::Build)).await;
        let head = status.clean_head();
        let selector = system_selector(config, head).await?;
        // The result link keeps the store path alive for subsequent Apply actions.
        let directory = tempfile::Builder::new()
            .prefix("nixos-manager-build-")
            .tempdir()
            .map_err(|_| Error::BuildDirectory)?;
        let mut command = Command::new(NIX);
        command
            .args([
                "--extra-experimental-features",
                "nix-command flakes",
                "build",
                "-L",
                "--no-write-lock-file",
                "--out-link",
            ])
            .arg(directory.path().join(OUT_LINK))
            .arg(selector);
        execute(runner, command, false, &mut interrupt, &mut emit).await?;
        let path = Build::result_path(&directory)
            .await
            .ok_or(Error::BuildResult)?;
        let build = Arc::new(Build {
            head: head.map(str::to_owned),
            directory,
            path,
        });
        if head.is_some() {
            emit(Event::Built(Arc::clone(&build))).await;
        }
        build
    };

    if steps.boot {
        emit(Event::Started(Step::Register)).await;
        let mut command = privileged(NIX_ENV);
        command
            .args(["--profile", SYSTEM_PROFILE, "--set"])
            .arg(&build.path);
        execute(runner, command, true, &mut interrupt, &mut emit).await?;
    }
    let (step, mode) = match (steps.boot, steps.activate) {
        (false, false) => return Ok(()),
        (true, false) => (Step::Boot, "boot"),
        (false, true) => (Step::Activate, "test"),
        (true, true) => (Step::BootAndActivate, "switch"),
    };
    emit(Event::Started(step)).await;
    let mut command = privileged(build.path.join(SWITCH));
    command.arg(mode);
    execute(runner, command, true, &mut interrupt, &mut emit).await
}

fn privileged(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(PKEXEC);
    command
        // pkexec validates an inherited SHELL against /etc/shells.
        .env_remove("SHELL")
        .arg("--disable-internal-agent")
        .arg(program);
    command
}

async fn execute<F: Future<Output = ()> + Send>(
    runner: &CommandRunner,
    command: Command,
    privileged: bool,
    interrupt: &mut mpsc::Receiver<()>,
    emit: &mut (impl FnMut(Event) -> F + Send),
) -> Result<(), Error> {
    match runner
        .interruptible(command, interrupt, || emit(Event::InterruptFailed))
        .await
    {
        Ok(status) => match status.code() {
            Some(0) => Ok(()),
            Some(126) if privileged => Err(Error::Cancelled),
            Some(127) if privileged => Err(Error::AuthorizationFailed),
            Some(code) => Err(Error::Exited(code)),
            None => Err(Error::Interrupted),
        },
        Err(RunError::Interrupted) => Err(Error::Interrupted),
        Err(RunError::Launch) => Err(Error::LaunchFailed),
        Err(RunError::Wait) => Err(Error::WaitFailed),
    }
}

async fn system_selector(config: &Config, head: Option<&str>) -> Result<OsString, Error> {
    let mut selector = flake_reference(&config.path, &config.flake_directory, head).await?;
    let system = rustix::system::uname();
    let hostname = system.nodename().to_bytes();
    if hostname.is_empty() {
        return Err(Error::MissingHostname);
    }
    selector.push("#nixosConfigurations.\"");
    selector.push(OsStr::from_bytes(hostname));
    selector.push("\".config.system.build.toplevel");
    Ok(selector)
}

async fn flake_reference(
    repository: &Path,
    directory: &Path,
    head: Option<&str>,
) -> Result<OsString, Error> {
    let root = fs::canonicalize(repository)
        .await
        .map_err(|_| Error::InvalidPath)?;
    let path = fs::canonicalize(root.join(directory))
        .await
        .map_err(|_| Error::MissingFlake)?;
    let relative = path.strip_prefix(&root).map_err(|_| Error::InvalidPath)?;
    let file = fs::canonicalize(path.join("flake.nix"))
        .await
        .map_err(|_| Error::MissingFlake)?;
    if !file.starts_with(&root) {
        return Err(Error::InvalidPath);
    }
    if !fs::metadata(file)
        .await
        .is_ok_and(|metadata| metadata.is_file())
    {
        return Err(Error::MissingFlake);
    }

    let path_bytes = path.as_os_str().as_bytes();
    if head.is_none() && !path_bytes.contains(&b'#') && !path_bytes.contains(&b'?') {
        return Ok(path.into_os_string());
    }

    // Pin a clean build to the inspected commit even if the worktree changes during evaluation.
    let mut url = Url::from_file_path(&root).map_err(|_| Error::InvalidPath)?;
    let mut parameters = Vec::new();
    if !relative.as_os_str().is_empty() {
        let encoded: String =
            url::form_urlencoded::byte_serialize(relative.as_os_str().as_bytes()).collect();
        parameters.push(format!("dir={}", encoded.replace('+', "%20")));
    }
    if let Some(head) = head {
        parameters.push(format!("rev={head}"));
    }
    url.set_query(Some(&parameters.join("&")));
    Ok(format!("git+{url}").into())
}
