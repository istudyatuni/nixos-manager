//! Shared protocol between the GUI and the privileged apply helper.

use std::str::FromStr;

pub const NIX_ENV: &str = "/run/current-system/sw/bin/nix-env";
pub const SYSTEM_PROFILE: &str = "/nix/var/nix/profiles/system";
pub const STORE: &str = "/nix/store";
pub const SWITCH: &str = "bin/switch-to-configuration";

pub const READY: &[u8] = b"nixos-manager:ready";
pub const REGISTERED: &[u8] = b"nixos-manager:registered";
pub const APPLY: &[u8] = b"apply";

#[derive(Clone, Copy)]
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
