use std::{
    borrow::Cow,
    env, fs,
    io::{self, Write},
    os::unix::fs::PermissionsExt,
    path::{self, Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use url::Url;

use crate::i18n::LanguagePreference;

pub const CONFIG_DIRECTORY: &str = ".config/nixos-manager";
pub const DEFAULT_REPO_PATH: &str = "~/.config/nixos-manager/repo";
pub const DEFAULT_FLAKE_DIRECTORY: &str = ".";

#[derive(Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)]
    pub language: LanguagePreference,
    pub repository: String,
    pub token: String,
    pub path: PathBuf,
    pub branch: String,
    #[serde(default = "default_flake_directory")]
    pub flake_directory: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            language: LanguagePreference::default(),
            repository: String::new(),
            token: String::new(),
            path: DEFAULT_REPO_PATH.into(),
            branch: String::new(),
            flake_directory: default_flake_directory(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SaveError {
    Repository,
    Token,
    Path,
    Branch,
    FlakeDirectory,
    Write,
}

#[derive(Debug, Clone)]
pub struct LoadError;

pub fn load() -> Result<Option<Config>, LoadError> {
    let home = env::home_dir().ok_or(LoadError)?;
    let file = home.join(CONFIG_DIRECTORY).join("config.json");
    let bytes = match fs::read(file) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(LoadError),
    };

    let mut config: Config = serde_json::from_slice(&bytes).map_err(|_| LoadError)?;
    config.validate(&home, false).map_err(|_| LoadError)?;
    Ok(Some(config))
}

pub fn save(mut config: Config, repository_required: bool) -> Result<Config, SaveError> {
    let home = env::home_dir().ok_or(SaveError::Write)?;
    config.validate(&home, repository_required)?;

    let directory = home.join(CONFIG_DIRECTORY);
    fs::create_dir_all(&directory).map_err(|_| SaveError::Write)?;
    // NamedTempFile creates files with mode 0600 on Unix.
    let mut file = NamedTempFile::new_in(&directory).map_err(|_| SaveError::Write)?;
    file.as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|_| SaveError::Write)?;
    serde_json::to_writer_pretty(&mut file, &config).map_err(|_| SaveError::Write)?;
    file.write_all(b"\n").map_err(|_| SaveError::Write)?;
    file.as_file().sync_all().map_err(|_| SaveError::Write)?;
    file.persist(directory.join("config.json"))
        .map_err(|_| SaveError::Write)?;

    Ok(config)
}

impl Config {
    pub fn repository_path(&self) -> Option<&Path> {
        (!self.repository.is_empty()).then_some(self.path.as_path())
    }

    fn validate(&mut self, home: &Path, repository_required: bool) -> Result<(), SaveError> {
        self.flake_directory = relative_flake_directory(&self.flake_directory)?;

        // Language can be saved before a repository is set up.
        if !repository_required
            && self.repository.is_empty()
            && self.token.is_empty()
            && self.branch.is_empty()
        {
            self.path = absolute_repo_path(&self.path, home)?;
            return Ok(());
        }

        self.repository = github_repository(self.repository.trim()).ok_or(SaveError::Repository)?;

        let token = self.token.trim();
        if token.is_empty() || token.chars().any(char::is_whitespace) || token.contains('\0') {
            return Err(SaveError::Token);
        }
        self.token = token.to_owned();

        self.path = absolute_repo_path(&self.path, home)?;

        let branch = self.branch.trim();
        if !valid_branch(branch) {
            return Err(SaveError::Branch);
        }
        self.branch = branch.to_owned();
        Ok(())
    }
}

fn default_flake_directory() -> PathBuf {
    DEFAULT_FLAKE_DIRECTORY.into()
}

fn relative_flake_directory(input: &Path) -> Result<PathBuf, SaveError> {
    if input.as_os_str().as_encoded_bytes().contains(&0) {
        return Err(SaveError::FlakeDirectory);
    }

    let mut directory = PathBuf::new();
    for component in input.components() {
        match component {
            Component::Normal(name) => directory.push(name),
            Component::CurDir => {}
            _ => return Err(SaveError::FlakeDirectory),
        }
    }
    if directory.as_os_str().is_empty() {
        return Ok(default_flake_directory());
    }
    Ok(directory)
}

pub fn display_repo_path<'a>(path: &'a Path, home: Option<&Path>) -> Cow<'a, Path> {
    let Some(relative) = home.and_then(|home| path.strip_prefix(home).ok()) else {
        return Cow::Borrowed(path);
    };

    let tilde = Path::new("~");
    if relative.as_os_str().is_empty() {
        Cow::Borrowed(tilde)
    } else {
        Cow::Owned(tilde.join(relative))
    }
}

fn github_repository(input: &str) -> Option<String> {
    let url = github_url(input)?;
    let mut segments = url.path_segments()?;
    let owner = segments.next()?;
    let repo = segments.next()?;
    let name = repo.strip_suffix(".git").unwrap_or(repo);
    let valid_owner = !owner.is_empty()
        && owner
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        && !owner.starts_with('-')
        && !owner.ends_with('-');
    let valid_repo = !matches!(name, "" | "." | "..")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));

    (valid_owner && valid_repo).then(|| format!("https://github.com/{owner}/{repo}"))
}

pub fn github_url(input: &str) -> Option<Url> {
    let url = Url::parse(input).ok()?;
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }

    Some(url)
}

fn absolute_repo_path(input: &Path, home: &Path) -> Result<PathBuf, SaveError> {
    if input.as_os_str().is_empty() || input.as_os_str().as_encoded_bytes().contains(&0) {
        return Err(SaveError::Path);
    }

    if let Ok(relative) = input.strip_prefix("~") {
        return Ok(home.join(relative));
    }

    path::absolute(input).map_err(|_| SaveError::Path)
}

fn valid_branch(branch: &str) -> bool {
    // These are the Git branch-name rules for literal names.
    branch != "HEAD"
        && !branch.starts_with('-')
        && !branch.ends_with('.')
        && !branch.contains("..")
        && !branch.contains("@{")
        && !branch
            .bytes()
            .any(|byte| byte <= b' ' || byte == 127 || b"~^:?*[\\".contains(&byte))
        && branch
            .split('/')
            .all(|part| !part.is_empty() && !part.starts_with('.') && !part.ends_with(".lock"))
}
