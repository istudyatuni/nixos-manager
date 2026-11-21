use std::{
    fs::File,
    io::{self, Write},
    process::{ExitStatus, Output, Stdio},
};

use rustix::{
    process::{ioctl_tiocsctty, setsid},
    pty::{OpenptFlags, ioctl_tiocgptpeer, openpt, unlockpt},
    termios::{LocalModes, OptionalActions, SpecialCodeIndex, tcgetattr, tcsetattr},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    net::unix::pipe,
    process::{Child, Command},
    sync::mpsc,
};

const INTERRUPT_CHAR: u8 = 3;

#[derive(Clone)]
pub struct LogLine {
    pub generation: u64,
    pub text: String,
}

#[derive(Clone)]
pub struct CommandRunner {
    journal: mpsc::Sender<LogLine>,
    generation: u64,
    secret: String,
}

pub enum RunError {
    Launch,
    Wait,
    Interrupted,
}

impl CommandRunner {
    pub fn new(journal: mpsc::Sender<LogLine>) -> Self {
        Self {
            journal,
            generation: 0,
            secret: String::new(),
        }
    }

    pub fn with_secret(mut self, secret: String) -> Self {
        self.secret = secret;
        self
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn next_generation(&mut self) {
        self.generation += 1;
    }

    pub fn redact(&self, text: &str) -> String {
        if self.secret.is_empty() {
            text.to_owned()
        } else {
            text.replace(&self.secret, "***")
        }
    }

    async fn log(&self, text: String) {
        let _ = self
            .journal
            .send(LogLine {
                generation: self.generation,
                text,
            })
            .await;
    }

    async fn log_command(&self, command: &Command) {
        let command = command.as_std();
        let mut line = String::from("$");
        for word in std::iter::once(command.get_program()).chain(command.get_args()) {
            let word = self.redact(&word.to_string_lossy());
            line.push(' ');
            if !word.is_empty()
                && word
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"/-_.:+=,@%".contains(&byte))
            {
                line.push_str(&word);
            } else {
                line.push_str(&format!("{word:?}"));
            }
        }
        self.log(line).await;
    }

    /// Streams both pipes while retaining their original bytes for callers that parse stdout.
    pub async fn output(&self, command: &mut Command) -> io::Result<Output> {
        self.log_command(command).await;
        let result = self.capture(command).await;
        if let Err(error) = &result {
            self.log(self.redact(&error.to_string())).await;
        }
        result
    }

    async fn capture(&self, command: &mut Command) -> io::Result<Output> {
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let mut out = Vec::new();
        let mut err = Vec::new();
        let (status, (), ()) = tokio::try_join!(
            child.wait(),
            self.read_output(stdout, Some(&mut out)),
            self.read_output(stderr, Some(&mut err)),
        )?;
        Ok(Output {
            status,
            stdout: out,
            stderr: err,
        })
    }

    pub async fn interruptible<F: Future<Output = ()> + Send>(
        &self,
        command: Command,
        mut interrupt: mpsc::Receiver<()>,
        mut interrupt_failed: impl FnMut() -> F + Send,
    ) -> Result<ExitStatus, RunError> {
        if interrupt.try_recv().is_ok() {
            return Err(RunError::Interrupted);
        }
        self.log_command(&command).await;
        let (mut terminal, mut child, output) = match spawn_interruptible(command) {
            Ok(process) => process,
            Err(error) => {
                self.log(self.redact(&error.to_string())).await;
                return Err(RunError::Launch);
            }
        };

        let wait = async {
            let mut interrupt_sent = false;
            loop {
                tokio::select! {
                    result = child.wait() => break (result, interrupt_sent),
                    Some(()) = interrupt.recv(), if !interrupt_sent => {
                        match terminal.write_all(&[INTERRUPT_CHAR]) {
                            Ok(()) => interrupt_sent = true,
                            Err(error) => {
                                self.log(self.redact(&error.to_string())).await;
                                interrupt_failed().await;
                            }
                        }
                    }
                }
            }
        };
        let read = async {
            if let Err(error) = self.read_output(output, None).await {
                self.log(self.redact(&error.to_string())).await;
            }
        };
        let ((status, interrupt_sent), ()) = tokio::join!(wait, read);
        match status {
            Ok(status) if interrupt_sent && !status.success() => Err(RunError::Interrupted),
            Ok(status) => Ok(status),
            Err(error) => {
                self.log(self.redact(&error.to_string())).await;
                Err(RunError::Wait)
            }
        }
    }

    async fn read_output(
        &self,
        reader: impl AsyncRead + Unpin,
        mut capture: Option<&mut Vec<u8>>,
    ) -> io::Result<()> {
        let mut reader = BufReader::new(reader);
        let mut line = Vec::new();
        let mut after_cr = false;
        loop {
            let bytes = reader.fill_buf().await?;
            if bytes.is_empty() {
                break;
            }
            let end = bytes.iter().position(|&byte| matches!(byte, b'\n' | b'\r'));
            let (part, ending) = match end {
                Some(index) => (&bytes[..index], Some(bytes[index])),
                None => (bytes, None),
            };
            let consumed = part.len() + usize::from(ending.is_some());
            if let Some(capture) = &mut capture {
                capture.extend_from_slice(&bytes[..consumed]);
            }
            line.extend_from_slice(part);
            reader.consume(consumed);

            // Git reports progress with carriage returns. Treat CRLF as one newline.
            if ending.is_some() && !(after_cr && line.is_empty() && ending == Some(b'\n')) {
                self.log_output_line(&line).await;
                line.clear();
            }
            after_cr = ending == Some(b'\r');
        }
        if !line.is_empty() {
            self.log_output_line(&line).await;
        }
        Ok(())
    }

    async fn log_output_line(&self, bytes: &[u8]) {
        let line = self.redact(&String::from_utf8_lossy(bytes));
        // Porcelain Git output uses NUL separators.
        self.log(line.replace('\0', "\\0")).await;
    }
}

fn spawn_interruptible(mut command: Command) -> io::Result<(File, Child, pipe::Receiver)> {
    let terminal = attach_terminal(&mut command)?;
    // A shared pipe preserves write order across stdout and stderr.
    let (reader, writer) = io::pipe()?;
    let output = pipe::Receiver::from_owned_fd(reader.into())?;
    let child = command
        .stdin(Stdio::null())
        .stdout(writer.try_clone()?)
        .stderr(writer)
        .spawn()?;
    Ok((terminal, child, output))
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
