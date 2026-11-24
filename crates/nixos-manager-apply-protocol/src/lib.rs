//! Shared protocol between the GUI and the privileged apply helper.

use std::{
    fs, io,
    os::unix::{ffi::OsStrExt, fs::PermissionsExt},
    path::{Path, PathBuf},
    str::FromStr,
};

pub mod system;

pub const NIX_ENV: &str = "/run/current-system/sw/bin/nix-env";
pub const SYSTEM_PROFILE: &str = "/nix/var/nix/profiles/system";
pub const STORE: &str = "/nix/store";
pub const SWITCH: &str = "bin/switch-to-configuration";

pub const READY: &[u8] = b"nixos-manager:ready";
pub const REGISTERED: &[u8] = b"nixos-manager:registered";
pub const APPLY: &[u8] = b"apply";
pub const DELETE_GENERATIONS: &str = "delete-generations";
pub const SELECTION_CHANGED: u8 = 3;

pub fn delete_generation_args(
    numbers: impl IntoIterator<Item = u64>,
) -> impl Iterator<Item = String> {
    ["--profile", SYSTEM_PROFILE, "--delete-generations"]
        .into_iter()
        .map(str::to_owned)
        .chain(numbers.into_iter().map(|number| number.to_string()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Boot,
    Activate,
    BootAndActivate,
}

impl Mode {
    pub fn argument(self) -> &'static str {
        match self {
            Self::Boot => "boot",
            Self::Activate => "test",
            Self::BootAndActivate => "switch",
        }
    }

    pub fn activates(self) -> bool {
        !matches!(self, Self::Boot)
    }

    pub fn registers_generation(self) -> bool {
        !matches!(self, Self::Activate)
    }
}

impl FromStr for Mode {
    type Err = ();

    fn from_str(argument: &str) -> Result<Self, Self::Err> {
        match argument {
            "boot" => Ok(Self::Boot),
            "test" => Ok(Self::Activate),
            "switch" => Ok(Self::BootAndActivate),
            _ => Err(()),
        }
    }
}

pub fn system_script(system: &Path) -> io::Result<PathBuf> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "invalid NixOS system output");
    let name = system.file_name().ok_or_else(invalid)?.as_bytes();
    let (hash, name) = name.split_at_checked(32).ok_or_else(invalid)?;
    let name = name.strip_prefix(b"-").ok_or_else(invalid)?;
    if system.parent() != Some(Path::new(STORE))
        || !hash
            .iter()
            .all(|byte| b"0123456789abcdfghijklmnpqrsvwxyz".contains(byte))
        || name.is_empty()
        || !name
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || b"+._?=-".contains(byte))
        || fs::canonicalize(system)? != system
        || !system.join("nixos-version").is_file()
    {
        return Err(invalid());
    }

    let script = fs::canonicalize(system.join(SWITCH))?;
    let metadata = fs::metadata(&script)?;
    if !script.starts_with(STORE)
        || !metadata.is_file()
        || metadata.permissions().mode() & 0o111 == 0
    {
        return Err(invalid());
    }
    Ok(script)
}
