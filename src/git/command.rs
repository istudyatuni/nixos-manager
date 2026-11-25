use std::io;

use tokio::process::Command;

use crate::command_runner::CommandRunner;

use super::Error;

pub async fn available(runner: &CommandRunner) -> bool {
    runner
        .output(command().arg("--version"))
        .await
        .is_ok_and(|output| output.status.success())
}

pub(super) fn command() -> Command {
    let mut git = Command::new("git");
    for key in [
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_COMMON_DIR",
        "GIT_CONFIG",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
        "GIT_DIR",
        "GIT_GRAFT_FILE",
        "GIT_IMPLICIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_PREFIX",
        "GIT_REPLACE_REF_BASE",
        "GIT_SHALLOW_FILE",
        "GIT_WORK_TREE",
    ] {
        git.env_remove(key);
    }
    git
}

pub(super) async fn run(
    runner: &CommandRunner,
    command: &mut Command,
    failure: impl FnOnce(String) -> Error,
) -> Result<Vec<u8>, Error> {
    match runner.output(command).await {
        Ok(output) if output.status.success() => Ok(output.stdout),
        Ok(output) => Err(failure(
            runner.redact(String::from_utf8_lossy(&output.stderr).trim()),
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(Error::Unavailable),
        Err(error) => Err(failure(runner.redact(&error.to_string()))),
    }
}
