//! Privileged helper for applying NixOS configurations and deleting generations.
//!
//! The GUI starts this binary once through `pkexec`. Any required build runs
//! separately as the current user. The helper reports readiness on stdout and
//! waits for the Nix store path on stdin. For `boot` and `switch` it registers
//! the system generation and reports completion. It then waits for an `apply`
//! request before running `switch-to-configuration` with the selected mode.
//! EOF while waiting for input ends the helper.

use std::{
    env,
    ffi::OsStr,
    io::{self, BufRead, Read, Write},
    os::unix::{
        ffi::OsStrExt,
        process::{CommandExt, ExitStatusExt},
    },
    path::Path,
    process::{Command, ExitCode, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use nixos_manager_apply_protocol::{
    APPLY, DELETE_GENERATIONS, Mode, NIX_ENV, READY, REGISTERED, SELECTION_CHANGED, SYSTEM_PROFILE,
    delete_generation_args,
    system::{Generation, State},
    system_script,
};
use signal_hook::{consts::SIGINT, flag};

fn main() -> ExitCode {
    let mut args = env::args_os().skip(1);
    let argument = args.next();
    if argument.as_deref() == Some(OsStr::new(DELETE_GENERATIONS)) {
        return result_code(delete_generations(args));
    }
    let mode = argument
        .as_deref()
        .and_then(OsStr::to_str)
        .and_then(|mode| mode.parse().ok());
    let Some(mode) = mode.filter(|_| args.next().is_none()) else {
        return ExitCode::from(2);
    };

    // Let the registration command receive Ctrl+C directly, then reap it before exiting.
    let interrupted = Arc::new(AtomicBool::new(false));
    result_code(
        flag::register(SIGINT, Arc::clone(&interrupted)).and_then(|_| apply(mode, &interrupted)),
    )
}

fn result_code(result: io::Result<u8>) -> ExitCode {
    match result {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("nixos-manager-apply: {error}");
            ExitCode::FAILURE
        }
    }
}

fn delete_generations(mut args: impl Iterator<Item = std::ffi::OsString>) -> io::Result<u8> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "invalid generation selection");
    let mut generations = Vec::new();
    while let Some(number) = args.next() {
        let number = number
            .to_str()
            .and_then(|number| number.parse().ok())
            .ok_or_else(invalid)?;
        let path = args.next().ok_or_else(invalid)?.into();
        generations.push(Generation { number, path });
    }
    if generations.is_empty() {
        return Err(invalid());
    }
    let expected = generations.iter().collect::<Vec<_>>();
    if !State::read().can_remove_generations(&expected) {
        return Ok(SELECTION_CHANGED);
    }
    let status = Command::new(NIX_ENV)
        .args(delete_generation_args(
            generations.iter().map(|entry| entry.number),
        ))
        .stdin(Stdio::null())
        .status()?;
    Ok(u8::from(!status.success()))
}

fn apply(mode: Mode, interrupted: &AtomicBool) -> io::Result<u8> {
    let mut input = io::stdin().lock();
    let mut frame = Vec::new();
    write_marker(READY)?;
    if !read_frame(&mut input, &mut frame)? {
        return Ok(0);
    }

    let system = Path::new(OsStr::from_bytes(&frame));
    let script = system_script(system)?;
    if interrupted.load(Ordering::Relaxed) {
        return Ok(130);
    }
    if mode.registers_generation() {
        let status = Command::new(NIX_ENV)
            .args(["--profile", SYSTEM_PROFILE, "--set"])
            .arg(system)
            .stdin(Stdio::null())
            .status()?;
        if interrupted.load(Ordering::Relaxed) {
            return Ok(130);
        }
        if !status.success() {
            return Ok(status
                .code()
                .unwrap_or_else(|| 128 + status.signal().unwrap_or(0))
                as u8);
        }
        // Start the marker on a new line even if nix-env emitted a partial line.
        println!();
        write_marker(REGISTERED)?;
    }

    if !read_frame(&mut input, &mut frame)? {
        return Ok(0);
    }
    if frame != APPLY {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid apply request",
        ));
    }
    if interrupted.load(Ordering::Relaxed) {
        return Ok(130);
    }
    Err(Command::new(script)
        .arg(mode.argument())
        .stdin(Stdio::null())
        .exec())
}

fn read_frame(input: &mut impl BufRead, frame: &mut Vec<u8>) -> io::Result<bool> {
    frame.clear();
    if input.take(4096).read_until(b'\n', frame)? == 0 {
        return Ok(false);
    }
    if frame.pop() == Some(b'\n') {
        Ok(true)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "incomplete apply request",
        ))
    }
}

fn write_marker(marker: &[u8]) -> io::Result<()> {
    let mut output = io::stdout().lock();
    output.write_all(marker)?;
    output.write_all(b"\n")?;
    output.flush()
}
