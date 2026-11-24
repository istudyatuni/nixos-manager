use std::{
    env, fs,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    process::Stdio,
};

use nixos_manager_apply_protocol::{
    APPLY, DELETE_GENERATIONS, Mode, NIX_ENV, READY, REGISTERED, SELECTION_CHANGED, STORE, SWITCH,
    SYSTEM_PROFILE, delete_generation_args, system::Generation,
};
use tokio::{process::Command, sync::mpsc};

use super::{Error, Event, PKEXEC, Step, check_status, executable};
use crate::command_runner::{CommandRunner, OutputError, RunningCommand};

pub(super) const HELPER_NAME: &str = "nixos-manager-apply";

fn helper_command(argument: &str) -> Result<Command, Error> {
    let mut command = Command::new(PKEXEC);
    command
        // pkexec validates an inherited SHELL against /etc/shells.
        .env_remove("SHELL")
        .arg("--disable-internal-agent")
        .arg(helper_path().ok_or(Error::LaunchFailed)?)
        .arg(argument);
    Ok(command)
}

pub(super) async fn delete_generations(
    runner: &CommandRunner,
    generations: &[&Generation],
) -> Result<(), Error> {
    let mut helper = helper_command(DELETE_GENERATIONS)?;
    for generation in generations {
        helper
            .arg(generation.number.to_string())
            .arg(&generation.path);
    }
    let mut command = Command::new(NIX_ENV);
    command.args(delete_generation_args(
        generations.iter().map(|entry| entry.number),
    ));
    runner.log_command(&command).await;
    let status = runner
        .output_with_error_kind(&mut helper)
        .await
        .map_err(|error| match error {
            OutputError::Launch(_) => Error::LaunchFailed,
            OutputError::Wait(_) => Error::WaitFailed,
        })?
        .status;
    match status.code() {
        Some(126) => Err(Error::Cancelled),
        Some(127) => Err(Error::AuthorizationFailed),
        Some(code) if code == i32::from(SELECTION_CHANGED) => Err(Error::TargetUnavailable),
        _ => check_status(status),
    }
}

pub(super) async fn authorize<F: Future<Output = ()> + Send>(
    runner: &CommandRunner,
    mode: Mode,
    interrupt: &mut mpsc::Receiver<()>,
    emit: &mut (impl FnMut(Event) -> F + Send),
) -> Result<RunningCommand, Error> {
    emit(Event::Started(Step::Authorize)).await;
    let mut command = helper_command(mode.argument())?;
    command.stdin(Stdio::piped());
    let mut helper = runner.start_interruptible(command, interrupt).await?;
    let ready = runner
        .wait_for_marker(&mut helper, READY, interrupt, || {
            emit(Event::InterruptFailed)
        })
        .await;
    if matches!(ready, Ok(true)) {
        return Ok(helper);
    }

    let status = runner
        .finish(helper, interrupt, || emit(Event::InterruptFailed))
        .await;
    ready?;
    match status?.code() {
        Some(126) => Err(Error::Cancelled),
        Some(127) => Err(Error::AuthorizationFailed),
        None => Err(Error::Interrupted),
        _ => Err(Error::WaitFailed),
    }
}

pub(super) async fn apply<F: Future<Output = ()> + Send>(
    runner: &CommandRunner,
    mut helper: RunningCommand,
    mode: Mode,
    path: &Path,
    interrupt: &mut mpsc::Receiver<()>,
    emit: &mut (impl FnMut(Event) -> F + Send),
    on_registered: impl FnOnce() -> Result<(), Error> + Send,
) -> Result<(), Error> {
    let transfer = async {
        let mut input = path.as_os_str().as_bytes().to_vec();
        input.push(b'\n');
        if mode.registers_generation() {
            emit(Event::Started(Step::Register)).await;
            let mut register = Command::new(NIX_ENV);
            register
                .args(["--profile", SYSTEM_PROFILE, "--set"])
                .arg(path);
            runner.log_command(&register).await;
            send(&mut helper, &input, interrupt).await?;
            if !runner
                .wait_for_marker(&mut helper, REGISTERED, interrupt, || {
                    emit(Event::InterruptFailed)
                })
                .await?
            {
                return Ok(false);
            }
            on_registered()?;
            input.clear();
        }

        let step = match mode {
            Mode::Boot => Step::Boot,
            Mode::Activate => Step::Activate,
            Mode::BootAndActivate => Step::BootAndActivate,
        };
        emit(Event::Started(step)).await;
        let mut activate = Command::new(path.join(SWITCH));
        activate.arg(mode.argument());
        runner.log_command(&activate).await;
        input.extend_from_slice(APPLY);
        input.push(b'\n');
        send(&mut helper, &input, interrupt).await?;
        Ok::<_, Error>(true)
    }
    .await;

    let status = runner
        .finish(helper, interrupt, || emit(Event::InterruptFailed))
        .await;
    let sent = transfer?;
    check_status(status?)?;
    if sent { Ok(()) } else { Err(Error::WaitFailed) }
}

async fn send(
    helper: &mut RunningCommand,
    bytes: &[u8],
    interrupt: &mut mpsc::Receiver<()>,
) -> Result<(), Error> {
    if interrupt.try_recv().is_ok() {
        return Err(Error::Interrupted);
    }
    helper.send(bytes).await.map_err(|_| Error::WaitFailed)
}

pub(super) fn helper_path() -> Option<PathBuf> {
    let current_exe = env::current_exe().ok()?;
    if current_exe.starts_with(STORE) {
        let package = current_exe.parent()?.parent()?;
        return ["libexec", "bin"]
            .into_iter()
            .map(|directory| package.join(directory).join(HELPER_NAME))
            .filter_map(|candidate| fs::canonicalize(candidate).ok())
            .find(|path| path.starts_with(STORE) && executable(path));
    }
    option_env!("NIXOS_MANAGER_DEV_HELPER")
        .map(PathBuf::from)
        .filter(|path| executable(path))
}
