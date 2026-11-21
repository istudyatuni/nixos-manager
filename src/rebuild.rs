use std::{
    collections::VecDeque,
    ffi::{OsStr, OsString},
    fs::File,
    io::{self, Write},
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    process::Stdio,
};

use rustix::{
    fs::{Access, access},
    process::{ioctl_tiocsctty, setsid},
    pty::{OpenptFlags, ioctl_tiocgptpeer, openpt, unlockpt},
    termios::{LocalModes, OptionalActions, SpecialCodeIndex, tcgetattr, tcsetattr},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::Command,
    sync::mpsc,
};
use url::Url;

use crate::git::{self, WorkingCopy};

pub const PKEXEC: &str = "/run/wrappers/bin/pkexec";
pub const NIXOS_REBUILD: &str = "/run/current-system/sw/bin/nixos-rebuild";
const LOG_CAPACITY: usize = 10_000;
const INTERRUPT_CHAR: u8 = 3;

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
    Lines(Vec<String>),
    InterruptFailed,
    Finished(Outcome),
}

pub struct Run {
    pub mode: Mode,
    pub outcome: Option<Outcome>,
    pub interrupting: bool,
    lines: VecDeque<String>,
}

impl Run {
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            outcome: None,
            interrupting: false,
            lines: VecDeque::new(),
        }
    }

    pub fn is_running(&self) -> bool {
        self.outcome.is_none()
    }

    /// Appends lines and returns the byte length of the removed prefix in the journal text.
    pub fn append(&mut self, lines: Vec<String>) -> usize {
        self.lines.extend(lines);
        let old_lines = self.lines.len().saturating_sub(LOG_CAPACITY);
        self.lines
            .drain(..old_lines)
            .map(|line| line.len() + 1)
            .sum()
    }

    pub fn has_lines(&self) -> bool {
        !self.lines.is_empty()
    }

    pub fn journal(&self) -> String {
        let mut output = String::new();
        for (index, line) in self.lines.iter().enumerate() {
            if index != 0 {
                output.push('\n');
            }
            output.push_str(line);
        }
        output
    }
}

pub async fn run<F: Future<Output = ()> + Send>(
    path: PathBuf,
    repository: String,
    branch: String,
    flake_directory: PathBuf,
    mode: Mode,
    mut interrupt: mpsc::Receiver<()>,
    mut emit: impl FnMut(Event) -> F + Send,
) -> Outcome {
    let mut inspection = tokio::task::spawn_blocking(move || {
        let status = git::inspect(&path, &repository, &branch)?;
        let flake = if status.matches_revision {
            flake_reference(&path, &flake_directory)
        } else {
            Err(Outcome::Unsynced)
        };
        Ok((status, flake))
    });
    let check = tokio::select! {
        biased;
        Some(()) = interrupt.recv() => return Outcome::Interrupted,
        result = &mut inspection => result,
    };

    let (status, flake) = match check {
        Ok(Ok(inspection)) => inspection,
        failure => {
            emit(Event::Inspected(WorkingCopy::default())).await;
            match failure {
                Ok(Err(git::Error::Inspect(detail))) => emit(Event::Lines(vec![detail])).await,
                Err(error) => emit(Event::Lines(vec![error.to_string()])).await,
                _ => {}
            }
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
    execute(command, interrupt, emit).await
}

fn flake_reference(repository: &Path, directory: &Path) -> Result<OsString, Outcome> {
    let root = repository
        .canonicalize()
        .map_err(|_| Outcome::InvalidPath)?;
    let path = root
        .join(directory)
        .canonicalize()
        .map_err(|_| Outcome::MissingFlake)?;
    let relative = path.strip_prefix(&root).map_err(|_| Outcome::InvalidPath)?;
    let file = path
        .join("flake.nix")
        .canonicalize()
        .map_err(|_| Outcome::MissingFlake)?;
    if !file.starts_with(&root) {
        return Err(Outcome::InvalidPath);
    }
    if !file.is_file() {
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

async fn execute<F: Future<Output = ()> + Send>(
    mut command: Command,
    mut interrupt: mpsc::Receiver<()>,
    mut emit: impl FnMut(Event) -> F + Send,
) -> Outcome {
    if interrupt.try_recv().is_ok() {
        return Outcome::Interrupted;
    }
    let mut terminal = match attach_terminal(&mut command) {
        Ok(terminal) => terminal,
        Err(error) => {
            emit(Event::Lines(vec![error.to_string()])).await;
            return Outcome::LaunchFailed;
        }
    };
    let mut child = match command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            emit(Event::Lines(vec![error.to_string()])).await;
            return Outcome::LaunchFailed;
        }
    };

    const BATCH_SIZE: usize = 128;
    let (sender, mut receiver) = mpsc::channel(BATCH_SIZE);
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    tokio::spawn(read_lines(stdout, sender.clone()));
    tokio::spawn(read_lines(stderr, sender));

    let mut batch = Vec::new();
    let mut output_open = true;
    let mut interrupt_sent = false;
    let mut status = None;
    while output_open || status.is_none() {
        tokio::select! {
            result = child.wait(), if status.is_none() => status = Some(result),
            Some(()) = interrupt.recv(), if !interrupt_sent && status.is_none() => {
                match terminal.write_all(&[INTERRUPT_CHAR]) {
                    Ok(()) => interrupt_sent = true,
                    Err(error) => {
                        emit(Event::Lines(vec![error.to_string()])).await;
                        emit(Event::InterruptFailed).await;
                    }
                }
            }
            count = receiver.recv_many(&mut batch, BATCH_SIZE), if output_open => {
                if count == 0 {
                    output_open = false;
                } else {
                    emit(Event::Lines(std::mem::take(&mut batch))).await;
                }
            }
        }
    }

    match status.expect("the process has exited and both output streams have closed") {
        Ok(status) if interrupt_sent && !status.success() => Outcome::Interrupted,
        Ok(status) => match status.code() {
            Some(0) => Outcome::Success,
            Some(126) => Outcome::Cancelled,
            Some(127) => Outcome::AuthorizationFailed,
            Some(code) => Outcome::Exited(code),
            None => Outcome::Interrupted,
        },
        Err(error) => {
            emit(Event::Lines(vec![error.to_string()])).await;
            Outcome::WaitFailed
        }
    }
}

fn attach_terminal(command: &mut Command) -> io::Result<File> {
    let flags = OpenptFlags::RDWR | OpenptFlags::NOCTTY | OpenptFlags::CLOEXEC;
    let terminal = openpt(flags)?;
    unlockpt(&terminal)?;
    let peer = ioctl_tiocgptpeer(&terminal, flags)?;
    let mut settings = tcgetattr(&peer)?;
    settings.local_modes.insert(LocalModes::ISIG);
    settings.local_modes.remove(LocalModes::ECHO);
    settings.special_codes[SpecialCodeIndex::VINTR] = INTERRUPT_CHAR;
    tcsetattr(&peer, OptionalActions::Now, &settings)?;

    // SAFETY: The child hook performs only async-signal-safe syscalls on the
    // already-open PTY. Both descriptors are close-on-exec.
    unsafe {
        command.pre_exec(move || {
            setsid()?;
            ioctl_tiocsctty(&peer)?;
            Ok(())
        });
    }
    Ok(File::from(terminal))
}

async fn read_lines(reader: impl AsyncRead + Unpin, sender: mpsc::Sender<String>) {
    let mut lines = BufReader::new(reader).split(b'\n');
    loop {
        let line = match lines.next_segment().await {
            Ok(Some(bytes)) => {
                let bytes = bytes.strip_suffix(b"\r").unwrap_or(&bytes);
                String::from_utf8_lossy(bytes).into_owned()
            }
            Ok(None) => break,
            Err(error) => {
                let _ = sender.send(error.to_string()).await;
                break;
            }
        };
        if sender.send(line).await.is_err() {
            break;
        }
    }
}
