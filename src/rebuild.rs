use std::{
    env,
    ffi::{OsStr, OsString},
    io,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use nixos_manager_apply_protocol::{Mode, NIX_ENV, system_script};
use rustix::fs::{Access, access};
use tokio::{fs, process::Command, sync::mpsc};
use url::Url;

use crate::{
    command_runner::{CommandRunner, RunError, RunningCommand},
    config::Config,
    git::{self, WorkingCopy},
};

const PKEXEC: &str = "/run/wrappers/bin/pkexec";
const NIX: &str = "nix";
const NIX_STORE: &str = "/run/current-system/sw/bin/nix-store";

mod apply;
pub mod history;

pub use history::Build;

#[derive(Clone, Copy)]
pub struct Steps {
    pub boot: bool,
    pub activate: bool,
}

impl Default for Steps {
    fn default() -> Self {
        Self::ACTIVATE
    }
}

impl Steps {
    pub const ACTIVATE: Self = Self {
        boot: false,
        activate: true,
    };

    pub fn applies(self) -> bool {
        self.boot || self.activate
    }

    fn apply_mode(self) -> Option<Mode> {
        match (self.boot, self.activate) {
            (false, false) => None,
            (true, false) => Some(Mode::Boot),
            (false, true) => Some(Mode::Activate),
            (true, true) => Some(Mode::BootAndActivate),
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct Tools {
    nix: bool,
    pkexec: bool,
    apply_helper: bool,
    nix_env: bool,
}

impl Tools {
    pub fn detect() -> Self {
        Self {
            nix: env::var_os("PATH").is_some_and(|path| {
                env::split_paths(&path).any(|directory| executable(&directory.join(NIX)))
            }),
            pkexec: executable(Path::new(PKEXEC)),
            apply_helper: apply::helper_path().is_some(),
            nix_env: executable(Path::new(NIX_ENV)),
        }
    }

    pub fn missing(self, needs_build: bool, steps: Steps) -> impl Iterator<Item = &'static str> {
        [
            (needs_build && !self.nix, NIX),
            (steps.applies() && !self.pkexec, PKEXEC),
            (steps.applies() && !self.apply_helper, apply::HELPER_NAME),
            (steps.boot && !self.nix_env, NIX_ENV),
        ]
        .into_iter()
        .filter_map(|(missing, program)| missing.then_some(program))
    }

    pub fn missing_for_generation_removal(self) -> Option<&'static str> {
        self.missing(
            false,
            Steps {
                boot: true,
                activate: false,
            },
        )
        .next()
    }
}

fn executable(path: &Path) -> bool {
    path.is_file() && access(path, Access::EXEC_OK).is_ok()
}

impl Build {
    pub fn matches_worktree(&self, status: &WorkingCopy) -> bool {
        status.clean_head().is_some_and(|head| {
            matches!(&self.source, history::Source::Built { revision, .. } if revision.as_deref() == Some(head))
        })
    }
}

#[derive(Clone, Copy)]
pub enum Step {
    History,
    Inspect,
    Authorize,
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
    BuildResult,
    BuildOutdated,
    History,
    TargetUnavailable,
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
    pub ready: bool,
    pub step: Step,
    pub outcome: Option<Result<(), Error>>,
    pub interrupting: bool,
}

impl Run {
    pub fn new(steps: Steps, ready: bool) -> Self {
        Self {
            steps,
            ready,
            step: Step::History,
            outcome: None,
            interrupting: false,
        }
    }

    pub fn is_running(&self) -> bool {
        self.outcome.is_none()
    }
}

#[derive(Clone)]
pub enum ReadyTarget {
    Selected(PathBuf),
    Quick(history::QuickRollback),
}

impl ReadyTarget {
    pub fn path(&self) -> &Path {
        match self {
            Self::Selected(path) => path,
            Self::Quick(rollback) => &rollback.target,
        }
    }
}

pub enum Request {
    Repository {
        config: Box<Config>,
        steps: Steps,
        cached: Option<Arc<Build>>,
    },
    Ready(ReadyTarget),
}

pub async fn run<F: Future<Output = ()> + Send>(
    runner: &CommandRunner,
    request: Request,
    mut interrupt: mpsc::Receiver<()>,
    mut emit: impl FnMut(Event) -> F + Send,
) -> Result<(), Error> {
    let mut session = loop {
        if interrupt.try_recv().is_ok() {
            return Err(Error::Interrupted);
        }
        match tokio::task::spawn_blocking(history::Session::open).await {
            Ok(Ok(session)) => break session,
            Ok(Err(error)) if error.kind() == io::ErrorKind::WouldBlock => {
                tokio::select! {
                    Some(()) = interrupt.recv() => return Err(Error::Interrupted),
                    () = tokio::time::sleep(Duration::from_millis(150)) => {}
                }
            }
            Ok(Err(error)) => return Err(history_error(error)),
            Err(_) => return Err(Error::History),
        }
    };

    match request {
        Request::Repository {
            config,
            steps,
            cached,
        } => {
            rebuild(
                runner,
                &config,
                steps,
                cached,
                &mut session,
                &mut interrupt,
                &mut emit,
            )
            .await
        }
        Request::Ready(target) => {
            let snapshot = session.snapshot();
            let path = target.path();
            if matches!(&target, ReadyTarget::Quick(expected) if snapshot.quick_rollback.as_ref() != Some(expected))
                || !snapshot
                    .targets
                    .iter()
                    .any(|entry| entry.store_path == path && !entry.system.active)
            {
                return Err(Error::TargetUnavailable);
            }
            apply_ready(
                runner,
                &mut session,
                &mut None,
                Mode::Activate,
                path,
                &mut interrupt,
                &mut emit,
            )
            .await
        }
    }
}

async fn rebuild<F: Future<Output = ()> + Send>(
    runner: &CommandRunner,
    config: &Config,
    steps: Steps,
    cached: Option<Arc<Build>>,
    session: &mut history::Session,
    interrupt: &mut mpsc::Receiver<()>,
    emit: &mut (impl FnMut(Event) -> F + Send),
) -> Result<(), Error> {
    emit(Event::Started(Step::Inspect)).await;
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

    let mode = steps.apply_mode();
    if let Some(build) = cached {
        if !build.matches_worktree(&status) {
            return Err(Error::BuildOutdated);
        }
        if !session.cached(&build) {
            return Err(Error::BuildResult);
        }
        return match mode {
            Some(mode) => {
                apply_ready(
                    runner,
                    session,
                    &mut None,
                    mode,
                    &build.store_path,
                    interrupt,
                    emit,
                )
                .await
            }
            None => Ok(()),
        };
    }

    let head = status.clean_head();
    let selector = system_selector(config, head).await?;
    let source = history::Source::Built {
        revision: head.map(str::to_owned),
        branch: status.branch,
    };
    // Authorize before a combined run so a cancelled prompt does not start a build.
    let mut helper = match mode {
        Some(mode) => Some(apply::authorize(runner, mode, interrupt, emit).await?),
        None => None,
    };
    let result = match build(runner, session, selector, source, interrupt, emit).await {
        Ok(build) => match mode {
            Some(mode) => {
                apply_ready(
                    runner,
                    session,
                    &mut helper,
                    mode,
                    &build.store_path,
                    interrupt,
                    emit,
                )
                .await
            }
            None => Ok(()),
        },
        Err(error) => Err(error),
    };
    if let Some(helper) = helper {
        // EOF makes the helper exit if the target could not be handed off.
        let _ = runner
            .finish(helper, interrupt, || emit(Event::InterruptFailed))
            .await;
    }
    result
}

async fn build<F: Future<Output = ()> + Send>(
    runner: &CommandRunner,
    session: &mut history::Session,
    selector: OsString,
    source: history::Source,
    interrupt: &mut mpsc::Receiver<()>,
    emit: &mut (impl FnMut(Event) -> F + Send),
) -> Result<Arc<Build>, Error> {
    emit(Event::Started(Step::Build)).await;
    let root = session.start_build(source).map_err(history_error)?;
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
        .arg(&root)
        .arg(selector);
    let status = runner
        .interruptible(command, interrupt, || emit(Event::InterruptFailed))
        .await?;
    check_status(status)?;

    let path = history::resolve_system(&root).ok_or(Error::BuildResult)?;
    let build = Arc::new(session.finish_build(&path).map_err(history_error)?);
    if matches!(
        &build.source,
        history::Source::Built {
            revision: Some(_),
            ..
        }
    ) {
        emit(Event::Built(Arc::clone(&build))).await;
    }
    Ok(build)
}

async fn apply_ready<F: Future<Output = ()> + Send>(
    runner: &CommandRunner,
    session: &mut history::Session,
    helper: &mut Option<RunningCommand>,
    mode: Mode,
    path: &Path,
    interrupt: &mut mpsc::Receiver<()>,
    emit: &mut (impl FnMut(Event) -> F + Send),
) -> Result<(), Error> {
    system_script(path).map_err(|_| Error::BuildResult)?;
    let current = history::current_system();
    let to_pin = current
        .as_deref()
        .filter(|path| session.needs_pin(mode, path));
    session
        .start_operation(mode, path, current.clone())
        .map_err(history_error)?;
    let result = async {
        if let Some(previous) = to_pin {
            emit(Event::Started(Step::History)).await;
            let root = session
                .start_build(history::Source::Imported {
                    discovered_at: std::time::SystemTime::now().into(),
                })
                .map_err(history_error)?;
            let mut command = Command::new(NIX_STORE);
            command
                .args(["--realise", "--add-root"])
                .arg(&root)
                .arg(previous);
            let status = runner
                .interruptible(command, interrupt, || emit(Event::InterruptFailed))
                .await?;
            check_status(status)?;
            session.finish_build(previous).map_err(history_error)?;
        }
        let helper = match helper.take() {
            Some(helper) => helper,
            None => apply::authorize(runner, mode, interrupt, emit).await?,
        };
        apply::apply(runner, helper, mode, path, interrupt, emit, || {
            session.registered().map_err(history_error)
        })
        .await
    }
    .await;
    let outcome = match result {
        Ok(()) => history::Outcome::Succeeded,
        Err(Error::Cancelled) => history::Outcome::Cancelled,
        Err(Error::Interrupted) => history::Outcome::Interrupted,
        Err(_) => history::Outcome::Failed,
    };
    session
        .finish_operation(outcome, history::current_system())
        .map_err(history_error)?;
    result
}

fn history_error(error: io::Error) -> Error {
    eprintln!("nixos-manager: {error}");
    Error::History
}

fn check_status(status: std::process::ExitStatus) -> Result<(), Error> {
    match status.code() {
        Some(0) => Ok(()),
        Some(code) => Err(Error::Exited(code)),
        None => Err(Error::Interrupted),
    }
}

impl From<RunError> for Error {
    fn from(error: RunError) -> Self {
        match error {
            RunError::Interrupted => Self::Interrupted,
            RunError::Launch => Self::LaunchFailed,
            RunError::Wait => Self::WaitFailed,
        }
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
