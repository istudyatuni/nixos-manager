use std::{
    collections::VecDeque,
    ffi::{OsStr, OsString},
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    process::Stdio,
};

use rustix::fs::{Access, access};
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
    Finished(Outcome),
}

pub struct Run {
    pub mode: Mode,
    pub outcome: Option<Outcome>,
    lines: VecDeque<String>,
}

impl Run {
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            outcome: None,
            lines: VecDeque::new(),
        }
    }

    pub fn is_running(&self) -> bool {
        self.outcome.is_none()
    }

    pub fn append(&mut self, lines: Vec<String>) {
        self.lines.extend(lines);
        let old_lines = self.lines.len().saturating_sub(LOG_CAPACITY);
        self.lines.drain(..old_lines);
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
    mut emit: impl FnMut(Event) -> F + Send,
) -> Outcome {
    let check = tokio::task::spawn_blocking(move || {
        let status = git::inspect(&path, &repository, &branch)?;
        let flake = if status.matches_revision {
            flake_reference(&path, &flake_directory)
        } else {
            Err(Outcome::Unsynced)
        };
        Ok((status, flake))
    })
    .await;

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
    execute(command, emit).await
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
    mut emit: impl FnMut(Event) -> F + Send,
) -> Outcome {
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

    // The channel closes after both readers reach EOF and send their final lines.
    let mut batch = Vec::new();
    while receiver.recv_many(&mut batch, BATCH_SIZE).await != 0 {
        emit(Event::Lines(std::mem::take(&mut batch))).await;
    }

    match child.wait().await {
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
