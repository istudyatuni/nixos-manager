use std::{
    borrow::Cow,
    env, fs,
    io::{self, Write},
    os::unix::fs::PermissionsExt,
    path::{self, Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tempfile::NamedTempFile;
use url::{ParseError, Url};

use crate::i18n::LanguagePreference;

pub const CONFIG_DIRECTORY: &str = ".config/nixos-manager";
pub const DEFAULT_REPO_PATH: &str = "~/.config/nixos-manager/repo";
pub const DEFAULT_UI_SCALE: u16 = 100;
pub const UI_SCALES: [u16; 6] = [75, 90, DEFAULT_UI_SCALE, 110, 125, 150];

#[derive(Clone, Deserialize, PartialEq, Eq, Serialize)]
pub struct Config {
    #[serde(default)]
    pub language: LanguagePreference,
    #[serde(default = "default_ui_scale")]
    pub ui_scale: u16,
    pub repository: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub token: String,
    pub path: PathBuf,
    pub branch: String,
    #[serde(default)]
    pub flake_directory: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            language: LanguagePreference::default(),
            ui_scale: DEFAULT_UI_SCALE,
            repository: String::new(),
            token: String::new(),
            path: DEFAULT_REPO_PATH.into(),
            branch: String::new(),
            flake_directory: PathBuf::new(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SaveError {
    Repository(RepositoryUrlError),
    Token,
    Path,
    Branch,
    FlakeDirectory,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepositoryUrlError {
    InvalidUrl,
    RequiresHttps,
    InvalidDomain,
    InvalidPath,
}

#[derive(Debug, Clone)]
pub struct LoadError;

pub fn load() -> Result<Option<Config>, LoadError> {
    let home = env::home_dir().ok_or(LoadError)?;
    let file = home.join(CONFIG_DIRECTORY).join("config.json");
    let Some(mut config) = read_json::<Config>(&file).map_err(|_| LoadError)? else {
        return Ok(None);
    };
    config.validate(&home, false).map_err(|_| LoadError)?;
    Ok(Some(config))
}

pub fn save(mut config: Config, repository_required: bool) -> Result<Config, SaveError> {
    let home = env::home_dir().ok_or(SaveError::Write)?;
    config.validate(&home, repository_required)?;

    let directory = home.join(CONFIG_DIRECTORY);
    prepare_json(&directory, &config)
        .map_err(|_| SaveError::Write)?
        .persist(directory.join("config.json"))
        .map_err(|_| SaveError::Write)?;
    Ok(config)
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn prepare_json(directory: &Path, value: &impl Serialize) -> io::Result<NamedTempFile> {
    fs::create_dir_all(directory)?;
    // NamedTempFile creates files with mode 0600 on Unix.
    let mut file = NamedTempFile::new_in(directory)?;
    file.as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    file.as_file().sync_all()?;
    Ok(file)
}

impl Config {
    pub fn repository_path(&self) -> Option<&Path> {
        (!self.repository.is_empty()).then_some(self.path.as_path())
    }

    pub fn requires_repository(&self) -> bool {
        !self.repository.is_empty() || !self.token.is_empty() || !self.branch.is_empty()
    }

    fn validate(&mut self, home: &Path, repository_required: bool) -> Result<(), SaveError> {
        self.flake_directory = relative_flake_directory(&self.flake_directory)?;
        if !UI_SCALES.contains(&self.ui_scale) {
            self.ui_scale = DEFAULT_UI_SCALE;
        }

        // Interface preferences can be saved before a repository is set up.
        if !repository_required && !self.requires_repository() {
            self.path = absolute_repo_path(&self.path, home)?;
            return Ok(());
        }

        self.repository =
            github_repository(self.repository.trim()).map_err(SaveError::Repository)?;

        let token = self.token.trim();
        if !valid_token(token) {
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

const fn default_ui_scale() -> u16 {
    DEFAULT_UI_SCALE
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

pub fn github_repository(input: &str) -> Result<String, RepositoryUrlError> {
    let url = github_url(input)?;
    let mut segments = url.path_segments().ok_or(RepositoryUrlError::InvalidPath)?;
    let (Some(owner), Some(repo)) = (segments.next(), segments.next()) else {
        return Err(RepositoryUrlError::InvalidPath);
    };
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

    (valid_owner && valid_repo)
        .then(|| format!("https://github.com/{owner}/{repo}"))
        .ok_or(RepositoryUrlError::InvalidPath)
}

pub fn github_url(input: &str) -> Result<Url, RepositoryUrlError> {
    let url = Url::parse(input).map_err(|error| match error {
        ParseError::EmptyHost
        | ParseError::IdnaError
        | ParseError::InvalidDomainCharacter
        | ParseError::InvalidIpv4Address
        | ParseError::InvalidIpv6Address => RepositoryUrlError::InvalidDomain,
        _ => RepositoryUrlError::InvalidUrl,
    })?;
    if url.scheme() != "https" {
        return Err(RepositoryUrlError::RequiresHttps);
    }
    if url.host_str() != Some("github.com") {
        return Err(RepositoryUrlError::InvalidDomain);
    }
    if url.port().is_some() || !url.username().is_empty() || url.password().is_some() {
        return Err(RepositoryUrlError::InvalidUrl);
    }

    Ok(url)
}

pub fn absolute_repo_path(input: &Path, home: &Path) -> Result<PathBuf, SaveError> {
    if input.as_os_str().is_empty() || input.as_os_str().as_encoded_bytes().contains(&0) {
        return Err(SaveError::Path);
    }

    if let Ok(relative) = input.strip_prefix("~") {
        return Ok(home.join(relative));
    }

    path::absolute(input).map_err(|_| SaveError::Path)
}

pub fn valid_token(token: &str) -> bool {
    !token
        .chars()
        .any(|character| character.is_whitespace() || character == '\0')
}

pub fn valid_branch(branch: &str) -> bool {
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
