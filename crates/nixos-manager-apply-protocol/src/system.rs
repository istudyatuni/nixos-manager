use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

use crate::{SYSTEM_PROFILE, system_script};

const CURRENT_SYSTEM: &str = "/run/current-system";
const BOOTED_SYSTEM: &str = "/run/booted-system";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Generation {
    pub number: u64,
    pub path: PathBuf,
}

#[derive(Clone, Default)]
pub struct Target {
    pub generations: Vec<u64>,
    pub profile_selected: bool,
    pub selected_generation: Option<u64>,
    pub active: bool,
    pub booted: bool,
}

impl Target {
    pub fn protects_builds(&self) -> bool {
        self.profile_selected || self.active || self.booted
    }

    pub fn can_remove_generation(&self, number: u64, mut marked: impl FnMut(u64) -> bool) -> bool {
        self.generations.contains(&number)
            && (!self.profile_selected
                || self
                    .selected_generation
                    .is_some_and(|selected| selected != number))
            && (!(self.active || self.booted)
                || self
                    .generations
                    .iter()
                    .any(|&other| other != number && !marked(other)))
    }
}

pub struct State {
    pub targets: BTreeMap<PathBuf, Target>,
    pub current: Option<PathBuf>,
    pub complete: bool,
}

impl State {
    pub fn read() -> Self {
        Self::discover(
            Path::new(SYSTEM_PROFILE),
            Path::new(CURRENT_SYSTEM),
            Path::new(BOOTED_SYSTEM),
            resolve_system,
        )
    }

    pub fn discover(
        profile: &Path,
        current: &Path,
        booted: &Path,
        resolve: impl Fn(&Path) -> Option<PathBuf>,
    ) -> Self {
        let mut targets = BTreeMap::<PathBuf, Target>::new();
        let mut complete = true;
        let directory = profile.parent().expect("system profile has a parent");
        match fs::read_dir(directory) {
            Ok(entries) => {
                for entry in entries {
                    let Ok(entry) = entry else {
                        complete = false;
                        continue;
                    };
                    let path = entry.path();
                    if let Some(number) = generation_number(&path) {
                        match entry.file_type() {
                            Ok(kind) if kind.is_symlink() => {
                                if let Some(system) = resolve(&path) {
                                    targets.entry(system).or_default().generations.push(number);
                                }
                            }
                            Ok(_) => {}
                            Err(_) => complete = false,
                        }
                    }
                }
                for target in targets.values_mut() {
                    target.generations.sort_unstable_by(|a, b| b.cmp(a));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => complete = false,
        }
        let mut read_protected = |path: &Path| {
            let resolved = resolve(path);
            if resolved.is_none()
                && !matches!(fs::symlink_metadata(path), Err(error) if error.kind() == io::ErrorKind::NotFound)
            {
                complete = false;
            }
            resolved
        };
        if let Some(system) = read_protected(profile) {
            let target = targets.entry(system).or_default();
            target.profile_selected = true;
            target.selected_generation = fs::read_link(profile)
                .ok()
                .map(|link| directory.join(link))
                .filter(|link| link.parent() == Some(directory))
                .and_then(|link| generation_number(&link))
                .filter(|number| target.generations.contains(number));
        }
        let current = read_protected(current);
        if let Some(system) = &current {
            targets.entry(system.clone()).or_default().active = true;
        }
        if let Some(system) = read_protected(booted) {
            targets.entry(system).or_default().booted = true;
        }
        Self {
            targets,
            current,
            complete,
        }
    }

    pub fn can_remove_generations(&self, generations: &[&Generation]) -> bool {
        self.complete
            && generations.iter().all(|generation| {
                self.targets.get(&generation.path).is_some_and(|target| {
                    target.can_remove_generation(generation.number, |number| {
                        generations
                            .iter()
                            .any(|other| other.path == generation.path && other.number == number)
                    })
                })
            })
    }
}

pub fn current_system() -> Option<PathBuf> {
    resolve_system(Path::new(CURRENT_SYSTEM))
}

pub fn resolve_system(path: &Path) -> Option<PathBuf> {
    let system = fs::canonicalize(path).ok()?;
    system_script(&system).ok()?;
    Some(system)
}

fn generation_number(path: &Path) -> Option<u64> {
    let number = path
        .file_name()?
        .to_str()?
        .strip_prefix("system-")?
        .strip_suffix("-link")?;
    let canonical = !number.is_empty()
        && number.bytes().all(|byte| byte.is_ascii_digit())
        && (number == "0" || !number.starts_with('0'));
    canonical.then_some(number)?.parse().ok()
}
