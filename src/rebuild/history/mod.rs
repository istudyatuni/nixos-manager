use std::{
    collections::{BTreeMap, HashSet},
    env,
    fs::{self, File, OpenOptions},
    io,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    time::SystemTime,
};

use chrono::{DateTime, Utc};
use nixos_manager_apply_protocol::{
    Mode,
    system::{State as SystemState, Target as SystemTarget},
    system_script,
};
use rustix::fs::{FlockOperation, flock};
use serde::{Deserialize, Serialize};

use crate::config::{prepare_json, read_json};

const VERSION: u32 = 1;
const HISTORY_FILE: &str = "history.json";
const AUTO_ROOTS: &str = "/nix/var/nix/gcroots/auto";
mod removal;
pub(super) use nixos_manager_apply_protocol::system::{current_system, resolve_system};
pub use removal::{Removal, remove};

#[derive(Serialize, Deserialize)]
struct History {
    version: u32,
    builds: Vec<Build>,
    pending_build: Option<PendingBuild>,
    operations: Vec<Operation>,
}

impl Default for History {
    fn default() -> Self {
        Self {
            version: VERSION,
            builds: Vec::new(),
            pending_build: None,
            operations: Vec::new(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Build {
    id: String,
    pub store_path: PathBuf,
    gc_root: PathBuf,
    pub source: Source,
    pub built_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    Built {
        branch: Option<String>,
        revision: Option<String>,
    },
    Imported {
        discovered_at: DateTime<Utc>,
    },
}

#[derive(Serialize, Deserialize)]
struct PendingBuild {
    id: String,
    source: Source,
    started_at: DateTime<Utc>,
}

impl PendingBuild {
    fn gc_root(&self) -> io::Result<PathBuf> {
        result_link(&self.id)
    }

    fn complete(self, gc_root: PathBuf, store_path: PathBuf, time: DateTime<Utc>) -> Build {
        let built_at = matches!(self.source, Source::Built { .. }).then_some(time);
        Build {
            id: self.id,
            store_path,
            gc_root,
            built_at,
            source: self.source,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Operation {
    mode: ApplyMode,
    target: PathBuf,
    started_at: DateTime<Utc>,
    previous_active: Option<PathBuf>,
    active_after: Option<PathBuf>,
    registered: bool,
    outcome: Outcome,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum ApplyMode {
    #[serde(rename = "boot")]
    Boot,
    #[serde(rename = "test")]
    Test,
    #[serde(rename = "switch")]
    Switch,
}

impl From<Mode> for ApplyMode {
    fn from(mode: Mode) -> Self {
        match mode {
            Mode::Boot => Self::Boot,
            Mode::Activate => Self::Test,
            Mode::BootAndActivate => Self::Switch,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Outcome {
    Pending,
    Succeeded,
    Cancelled,
    Interrupted,
    Failed,
}

#[derive(Clone, Default)]
pub struct Snapshot {
    pub targets: Vec<Target>,
    pub quick_rollback: Option<QuickRollback>,
    pub unavailable: bool,
    switch_boot_target: Option<PathBuf>,
}

impl Snapshot {
    pub fn contains_build(&self, build: &Build) -> bool {
        self.targets
            .iter()
            .flat_map(|target| &target.builds)
            .any(|entry| entry.id == build.id && entry.store_path == build.store_path)
    }

    pub fn warns_about_switch(&self, target: &Path) -> bool {
        self.switch_boot_target
            .as_deref()
            .is_some_and(|boot| boot != target)
    }
}

#[derive(Clone)]
pub struct Target {
    pub store_path: PathBuf,
    pub builds: Vec<Build>,
    pub system: SystemTarget,
}

impl Target {
    fn new(store_path: &Path) -> Self {
        Self {
            store_path: store_path.to_owned(),
            builds: Vec::new(),
            system: SystemTarget::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuickRollback {
    pub target: PathBuf,
    previous_operation: DateTime<Utc>,
}

#[cfg(test)]
impl QuickRollback {
    pub(crate) fn for_test(target: impl Into<PathBuf>) -> Self {
        Self {
            target: target.into(),
            previous_operation: DateTime::UNIX_EPOCH,
        }
    }
}

pub(super) struct Session {
    directory: PathBuf,
    auto_roots: PathBuf,
    history: History,
    lock: File,
}

impl Session {
    pub(super) fn open() -> io::Result<Self> {
        Self::open_in(&state_directory()?, Path::new(AUTO_ROOTS))
    }

    fn open_in(directory: &Path, auto_roots: &Path) -> io::Result<Self> {
        fs::create_dir_all(directory)?;
        let directory = fs::canonicalize(directory)?;
        let lock = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .mode(0o600)
            .open(directory.join("history.lock"))?;
        flock(&lock, FlockOperation::NonBlockingLockExclusive)?;
        let mut session = Self {
            history: History::default(),
            directory,
            auto_roots: auto_roots.to_owned(),
            lock,
        };
        session.history = load(&session.directory)?;
        if session.history.reconcile(&session.directory, auto_roots)? {
            session.save()?;
        }
        Ok(session)
    }

    fn save(&self) -> io::Result<()> {
        prepare_json(&self.directory, &self.history)?.persist(self.directory.join(HISTORY_FILE))?;
        File::open(&self.directory)?.sync_all()
    }

    pub(super) fn cached(&self, build: &Build) -> bool {
        self.history
            .builds
            .iter()
            .any(|entry| entry.id == build.id && entry.store_path == build.store_path)
    }

    pub(super) fn needs_pin(&self, mode: Mode, path: &Path) -> bool {
        (mode.activates()
            || !self
                .history
                .operations
                .iter()
                .any(|operation| operation.outcome == Outcome::Succeeded || operation.registered))
            && !self
                .history
                .builds
                .iter()
                .any(|build| build.store_path == path)
    }

    pub(super) fn start_build(&mut self, source: Source) -> io::Result<PathBuf> {
        let (id, directory) = match self.reusable_build_id(&source)? {
            Some(id) => (id, None),
            None => {
                let builds = self.directory.join("builds");
                fs::create_dir_all(&builds)?;
                let directory = tempfile::Builder::new().prefix("gen-").tempdir_in(builds)?;
                let id = directory
                    .path()
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                (id, Some(directory))
            }
        };
        let pending = PendingBuild {
            id,
            source,
            started_at: SystemTime::now().into(),
        };
        let root = self.directory.join(pending.gc_root()?);
        self.history.pending_build = Some(pending);
        self.save()?;
        if let Some(directory) = directory {
            let _ = directory.keep();
        }
        Ok(root)
    }

    fn reusable_build_id(&self, source: &Source) -> io::Result<Option<String>> {
        let Source::Built {
            revision: Some(revision),
            ..
        } = source
        else {
            return Ok(None);
        };
        let builds = self.history.builds.iter().rev().filter(|build| {
            matches!(&build.source, Source::Built { revision: Some(previous), .. } if previous == revision)
        });
        if builds.clone().next().is_none() {
            return Ok(None);
        }
        let roots = registered_roots(&self.auto_roots)?;
        for build in builds {
            if resolve_root(&self.directory.join(&build.gc_root), &roots)?.as_ref()
                == Some(&build.store_path)
            {
                return Ok(Some(build.id.clone()));
            }
        }
        Ok(None)
    }

    pub(super) fn finish_build(&mut self, expected: &Path) -> io::Result<Build> {
        let pending = self
            .history
            .pending_build
            .take()
            .expect("build in progress");
        let gc_root = pending.gc_root()?;
        let roots = registered_roots(&self.auto_roots)?;
        if resolve_root(&self.directory.join(&gc_root), &roots)?.as_deref() != Some(expected) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "missing build GC root",
            ));
        }
        let build = pending.complete(gc_root, expected.to_owned(), SystemTime::now().into());
        self.history.builds.retain(|entry| entry.id != build.id);
        self.history.builds.push(build.clone());
        self.save()?;
        Ok(build)
    }

    pub(super) fn start_operation(
        &mut self,
        mode: Mode,
        target: &Path,
        current: Option<PathBuf>,
    ) -> io::Result<()> {
        self.history.operations.push(Operation {
            mode: mode.into(),
            target: target.to_owned(),
            started_at: SystemTime::now().into(),
            previous_active: current.filter(|_| mode.activates()),
            active_after: None,
            registered: false,
            outcome: Outcome::Pending,
        });
        self.save()
    }

    pub(super) fn registered(&mut self) -> io::Result<()> {
        self.history
            .operations
            .last_mut()
            .expect("operation in progress")
            .registered = true;
        self.save()
    }

    pub(super) fn finish_operation(
        &mut self,
        outcome: Outcome,
        active_after: Option<PathBuf>,
    ) -> io::Result<()> {
        let operation = self
            .history
            .operations
            .last_mut()
            .expect("operation in progress");
        operation.outcome = outcome;
        operation.active_after = active_after;
        self.save()
    }

    pub(super) fn snapshot(&self) -> Snapshot {
        self.history.snapshot()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // A subprocess forked by another thread can briefly inherit the file before exec.
        let _ = flock(&self.lock, FlockOperation::Unlock);
    }
}

impl History {
    fn reconcile(&mut self, directory: &Path, auto_roots: &Path) -> io::Result<bool> {
        let mut changed = false;
        if self.pending_build.is_some() || !self.builds.is_empty() {
            let roots = registered_roots(auto_roots)?;
            let original_count = self.builds.len();
            let mut retained = Vec::with_capacity(original_count);
            for build in self.builds.drain(..) {
                let gc_root = result_link(&build.id)?;
                if gc_root == build.gc_root
                    && resolve_root(&directory.join(gc_root), &roots)?.as_ref()
                        == Some(&build.store_path)
                {
                    retained.push(build);
                }
            }
            self.builds = retained;
            changed = self.builds.len() != original_count;

            if let Some(pending) = self.pending_build.take() {
                let gc_root = pending.gc_root()?;
                let root = directory.join(&gc_root);
                // A reused link that still points to the old result does not prove the rebuild finished.
                if !self.builds.iter().any(|build| build.id == pending.id)
                    && let Some(store_path) = resolve_root(&root, &roots)?
                {
                    let time = fs::symlink_metadata(&root)
                        .and_then(|metadata| metadata.modified())
                        .map_or(pending.started_at, DateTime::from);
                    self.builds
                        .push(pending.complete(gc_root, store_path, time));
                }
                changed = true;
            }
        }
        for operation in &mut self.operations {
            if operation.outcome == Outcome::Pending {
                operation.outcome = Outcome::Interrupted;
                changed = true;
            }
        }
        Ok(changed)
    }

    fn snapshot(&self) -> Snapshot {
        let SystemState {
            targets, current, ..
        } = SystemState::read();
        let mut targets: BTreeMap<_, _> = targets
            .into_iter()
            .map(|(store_path, system)| {
                let target = Target {
                    store_path: store_path.clone(),
                    builds: Vec::new(),
                    system,
                };
                (store_path, target)
            })
            .collect();
        for build in self.builds.iter().rev() {
            if targets.contains_key(&build.store_path) || system_script(&build.store_path).is_ok() {
                add_target(&mut targets, &build.store_path)
                    .builds
                    .push(build.clone());
            }
        }
        let quick_rollback = self.quick_rollback(current.as_deref(), &targets);
        let switch_boot_target = self
            .operations
            .iter()
            .rev()
            .find(|operation| operation.registered)
            .filter(|operation| {
                operation.mode == ApplyMode::Switch
                    && targets
                        .get(&operation.target)
                        .is_some_and(|target| target.system.profile_selected)
            })
            .or_else(|| {
                self.last_activation().filter(|operation| {
                    operation.mode == ApplyMode::Switch
                        && Some(operation.target.as_path()) == current.as_deref()
                })
            })
            .map(|operation| operation.target.clone());
        let mut targets: Vec<_> = targets.into_values().collect();
        targets.sort_by(|a, b| {
            (
                b.system.active,
                b.system.booted,
                b.builds.first().and_then(|build| build.built_at),
                b.system.generations.first(),
            )
                .cmp(&(
                    a.system.active,
                    a.system.booted,
                    a.builds.first().and_then(|build| build.built_at),
                    a.system.generations.first(),
                ))
        });
        Snapshot {
            targets,
            quick_rollback,
            unavailable: false,
            switch_boot_target,
        }
    }

    fn last_activation(&self) -> Option<&Operation> {
        self.operations.iter().rev().find(|operation| {
            operation.mode != ApplyMode::Boot
                && operation.previous_active.as_ref() != Some(&operation.target)
                && (operation.outcome == Outcome::Succeeded
                    || matches!(operation.outcome, Outcome::Failed | Outcome::Interrupted)
                        && operation.active_after.as_ref() == Some(&operation.target))
        })
    }

    fn quick_rollback(
        &self,
        current: Option<&Path>,
        targets: &BTreeMap<PathBuf, Target>,
    ) -> Option<QuickRollback> {
        let latest = self.last_activation()?;
        let previous = latest.previous_active.as_ref()?;
        (current == Some(&latest.target) && targets.contains_key(previous)).then(|| QuickRollback {
            target: previous.clone(),
            previous_operation: latest.started_at,
        })
    }
}

pub fn snapshot() -> Snapshot {
    match Session::open() {
        Ok(session) => session.snapshot(),
        Err(error) => {
            if error.kind() != io::ErrorKind::WouldBlock {
                eprintln!("nixos-manager: {error}");
            }
            Snapshot {
                unavailable: true,
                ..History::default().snapshot()
            }
        }
    }
}

fn add_target<'a>(targets: &'a mut BTreeMap<PathBuf, Target>, system: &Path) -> &'a mut Target {
    targets
        .entry(system.to_owned())
        .or_insert_with(|| Target::new(system))
}

fn state_directory() -> io::Result<PathBuf> {
    let path = env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| env::home_dir().map(|home| home.join(".local/state")))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no state directory"))?;
    Ok(path.join("nixos-manager"))
}

fn load(directory: &Path) -> io::Result<History> {
    let history = read_json::<History>(&directory.join(HISTORY_FILE))?.unwrap_or_default();
    if history.version != VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported build history version",
        ));
    }
    Ok(history)
}

fn result_link(id: &str) -> io::Result<PathBuf> {
    if id.strip_prefix("gen-").is_some_and(|suffix| {
        !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
    }) {
        Ok(Path::new("builds").join(id).join("result"))
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid build ID",
        ))
    }
}

fn registered_roots(directory: &Path) -> io::Result<HashSet<PathBuf>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(HashSet::new()),
        Err(error) => return Err(error),
    };
    let mut roots = HashSet::new();
    for entry in entries {
        match fs::read_link(entry?.path()) {
            Ok(link) if link.is_absolute() => {
                roots.insert(link);
            }
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::InvalidInput
                ) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(roots)
}

fn resolve_root(root: &Path, roots: &HashSet<PathBuf>) -> io::Result<Option<PathBuf>> {
    if !roots.contains(root) {
        return Ok(None);
    }
    let resolved = fs::read_link(root).and_then(|link| fs::canonicalize(root.with_file_name(link)));
    match resolved {
        Ok(path) => Ok(Some(path)),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound
                    | io::ErrorKind::InvalidInput
                    | io::ErrorKind::NotADirectory
            ) || error.raw_os_error() == Some(rustix::io::Errno::LOOP.raw_os_error()) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests;
