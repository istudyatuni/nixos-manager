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
    io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader},
    net::unix::pipe,
    process::{Child, Command},
    sync::mpsc,
};

const INTERRUPT_CHAR: u8 = 3;
pub const COMMAND_PROMPT: &str = "$ ";

#[derive(Clone)]
pub struct LogLine {
    pub generation: u64,
    pub text: String,
    pub is_command: bool,
}

#[derive(Clone)]
pub struct CommandRunner {
    journal: mpsc::Sender<LogLine>,
    generation: u64,
    secret: String,
    output_redactor: Option<fn(&mut String)>,
}

pub enum OutputError {
    Launch(io::Error),
    Wait(io::Error),
}

pub enum RunError {
    Launch,
    Wait,
    Interrupted,
}

pub struct RunningCommand {
    terminal: File,
    child: Child,
    output: BufReader<pipe::Receiver>,
    interrupted: bool,
}

impl RunningCommand {
    pub async fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.child
            .stdin
            .as_mut()
            .ok_or_else(|| io::Error::from(io::ErrorKind::BrokenPipe))?
            .write_all(bytes)
            .await
    }
}

impl CommandRunner {
    pub fn new(journal: mpsc::Sender<LogLine>) -> Self {
        Self {
            journal,
            generation: 0,
            secret: String::new(),
            output_redactor: None,
        }
    }

    pub fn with_secret(mut self, secret: String) -> Self {
        self.secret = secret;
        self
    }

    pub fn with_output_redactor(mut self, redact: fn(&mut String)) -> Self {
        self.output_redactor = Some(redact);
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
        self.send_log(text, false).await;
    }

    async fn send_log(&self, text: String, is_command: bool) {
        let _ = self
            .journal
            .send(LogLine {
                generation: self.generation,
                text,
                is_command,
            })
            .await;
    }

    pub async fn log_command(&self, command: &Command) {
        let command = command.as_std();
        let mut line = String::from(COMMAND_PROMPT);
        for (index, word) in std::iter::once(command.get_program())
            .chain(command.get_args())
            .enumerate()
        {
            let word = self.redact(&word.to_string_lossy());
            if index > 0 {
                line.push(' ');
            }
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
        self.send_log(line, true).await;
    }

    /// Streams both pipes while retaining their original bytes for callers that parse stdout.
    pub async fn output(&self, command: &mut Command) -> io::Result<Output> {
        self.output_with_error_kind(command)
            .await
            .map_err(|error| match error {
                OutputError::Launch(error) | OutputError::Wait(error) => error,
            })
    }

    /// Streams output and distinguishes launch failures from later I/O errors.
    pub async fn output_with_error_kind(
        &self,
        command: &mut Command,
    ) -> Result<Output, OutputError> {
        self.log_command(command).await;
        let result = self.capture(command).await;
        if let Err(OutputError::Launch(error) | OutputError::Wait(error)) = &result {
            self.log(self.redact(&error.to_string())).await;
        }
        result
    }

    async fn capture(&self, command: &mut Command) -> Result<Output, OutputError> {
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(OutputError::Launch)?;
        let mut stdout = BufReader::new(child.stdout.take().expect("piped stdout"));
        let mut stderr = BufReader::new(child.stderr.take().expect("piped stderr"));
        let mut out = Vec::new();
        let mut err = Vec::new();
        let (status, _, _) = tokio::try_join!(
            child.wait(),
            self.read_output(&mut stdout, Some(&mut out), None),
            self.read_output(&mut stderr, Some(&mut err), None),
        )
        .map_err(OutputError::Wait)?;
        Ok(Output {
            status,
            stdout: out,
            stderr: err,
        })
    }

    pub async fn interruptible<F: Future<Output = ()> + Send>(
        &self,
        mut command: Command,
        interrupt: &mut mpsc::Receiver<()>,
        interrupt_failed: impl FnMut() -> F + Send,
    ) -> Result<ExitStatus, RunError> {
        command.stdin(Stdio::null());
        let process = self.start_interruptible(command, interrupt).await?;
        self.finish(process, interrupt, interrupt_failed).await
    }

    pub async fn start_interruptible(
        &self,
        command: Command,
        interrupt: &mut mpsc::Receiver<()>,
    ) -> Result<RunningCommand, RunError> {
        if interrupt.try_recv().is_ok() {
            return Err(RunError::Interrupted);
        }
        self.log_command(&command).await;
        if interrupt.try_recv().is_ok() {
            return Err(RunError::Interrupted);
        }
        match spawn_interruptible(command) {
            Ok(process) => Ok(process),
            Err(error) => {
                self.log(self.redact(&error.to_string())).await;
                Err(RunError::Launch)
            }
        }
    }

    pub async fn wait_for_marker<F: Future<Output = ()> + Send>(
        &self,
        process: &mut RunningCommand,
        marker: &[u8],
        interrupt: &mut mpsc::Receiver<()>,
        mut interrupt_failed: impl FnMut() -> F + Send,
    ) -> Result<bool, RunError> {
        let read = self.read_output(&mut process.output, None, Some(marker));
        tokio::pin!(read);
        loop {
            tokio::select! {
                biased;
                Some(()) = interrupt.recv() => {
                    if self.signal_interrupt(&mut process.terminal).await {
                        process.interrupted = true;
                        return Err(RunError::Interrupted);
                    }
                    interrupt_failed().await;
                }
                result = &mut read => {
                    return match result {
                        Ok(found) => Ok(found),
                        Err(error) => {
                            self.log(self.redact(&error.to_string())).await;
                            Err(RunError::Wait)
                        }
                    };
                }
            }
        }
    }

    pub async fn finish<F: Future<Output = ()> + Send>(
        &self,
        process: RunningCommand,
        interrupt: &mut mpsc::Receiver<()>,
        mut interrupt_failed: impl FnMut() -> F + Send,
    ) -> Result<ExitStatus, RunError> {
        let RunningCommand {
            mut terminal,
            mut child,
            mut output,
            interrupted,
        } = process;
        drop(child.stdin.take());
        let wait = async {
            let mut interrupt_sent = interrupted;
            loop {
                tokio::select! {
                    result = child.wait() => break (result, interrupt_sent),
                    Some(()) = interrupt.recv(), if !interrupt_sent => {
                        interrupt_sent = self.signal_interrupt(&mut terminal).await;
                        if !interrupt_sent {
                            interrupt_failed().await;
                        }
                    }
                }
            }
        };
        let read = async {
            if let Err(error) = self.read_output(&mut output, None, None).await {
                self.log(self.redact(&error.to_string())).await;
            }
        };
        let ((status, interrupt_sent), ()) = tokio::join!(wait, read);
        match status {
            Ok(_) if interrupt_sent => Err(RunError::Interrupted),
            Ok(status) => Ok(status),
            Err(error) => {
                self.log(self.redact(&error.to_string())).await;
                Err(RunError::Wait)
            }
        }
    }

    async fn signal_interrupt(&self, terminal: &mut File) -> bool {
        match terminal.write_all(&[INTERRUPT_CHAR]) {
            Ok(()) => true,
            Err(error) => {
                self.log(self.redact(&error.to_string())).await;
                false
            }
        }
    }

    async fn read_output(
        &self,
        reader: &mut BufReader<impl AsyncRead + Unpin>,
        mut capture: Option<&mut Vec<u8>>,
        marker: Option<&[u8]>,
    ) -> io::Result<bool> {
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
                if marker == Some(line.as_slice()) {
                    return Ok(true);
                }
                self.log_output_line(&line).await;
                line.clear();
            }
            after_cr = ending == Some(b'\r');
        }
        if !line.is_empty() {
            self.log_output_line(&line).await;
        }
        Ok(false)
    }

    async fn log_output_line(&self, bytes: &[u8]) {
        let mut line = self.redact(&String::from_utf8_lossy(bytes));
        if let Some(redact) = self.output_redactor {
            redact(&mut line);
        }
        // Porcelain Git output uses NUL separators.
        self.log(line.replace('\0', "\\0")).await;
    }
}

fn spawn_interruptible(mut command: Command) -> io::Result<RunningCommand> {
    let terminal = attach_terminal(&mut command)?;
    // A shared pipe preserves write order across stdout and stderr.
    let (reader, writer) = io::pipe()?;
    let output = pipe::Receiver::from_owned_fd(reader.into())?;
    let child = command.stdout(writer.try_clone()?).stderr(writer).spawn()?;
    Ok(RunningCommand {
        terminal,
        child,
        output: BufReader::new(output),
        interrupted: false,
    })
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
