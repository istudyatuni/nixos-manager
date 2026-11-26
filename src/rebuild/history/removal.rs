use std::{collections::HashSet, io, path::PathBuf};

use nixos_manager_apply_protocol::system::Generation;
use tokio::task::spawn_blocking;

use super::{Build, Session, SystemState, Target, fs, registered_roots, resolve_root, result_link};
use crate::{
    command_runner::CommandRunner,
    rebuild::{Error, apply, history_error},
};

#[derive(Clone, PartialEq, Eq)]
pub enum Removal {
    Generation(Generation),
    Build { path: PathBuf, id: String },
}

impl Removal {
    pub fn generation(&self) -> Option<&Generation> {
        match self {
            Self::Generation(generation) => Some(generation),
            Self::Build { .. } => None,
        }
    }
}

impl Target {
    pub fn generation_removal(&self, number: u64) -> Option<Removal> {
        self.system
            .can_remove_generation(number, |_| false)
            .then(|| {
                Removal::Generation(Generation {
                    number,
                    path: self.store_path.clone(),
                })
            })
    }

    pub fn build_removal(&self, build: &Build) -> Option<Removal> {
        (!self.system.protects_builds()).then(|| Removal::Build {
            path: build.store_path.clone(),
            id: build.id.clone(),
        })
    }

    pub fn can_mark(&self, removal: &Removal, marked: &[Removal]) -> bool {
        removal.generation().is_none_or(|generation| {
            self.system
                .can_remove_generation(generation.number, |number| {
                    marked
                        .iter()
                        .filter_map(Removal::generation)
                        .any(|other| other.number == number && other.path == generation.path)
                })
        })
    }
}

pub async fn remove(runner: CommandRunner, selected: Vec<Removal>) -> Result<(), Error> {
    let (session, selected) = spawn_blocking(move || {
        let session = Session::open().map_err(history_error)?;
        let system = SystemState::read();
        let generations = selected
            .iter()
            .filter_map(Removal::generation)
            .collect::<Vec<_>>();
        if !system.can_remove_generations(&generations) {
            return Err(Error::TargetUnavailable);
        }
        session.selected_roots(&system, &selected)?;
        Ok((session, selected))
    })
    .await
    .map_err(|_| Error::History)??;

    let generations = selected
        .iter()
        .filter_map(Removal::generation)
        .collect::<Vec<_>>();
    if !generations.is_empty() {
        apply::delete_generations(&runner, &generations).await?;
    }
    if generations.len() == selected.len() {
        return Ok(());
    }

    spawn_blocking(move || {
        // Authorization can take time, during which another tool may activate a system.
        let roots = session.selected_roots(&SystemState::read(), &selected)?;
        remove_roots(roots)
    })
    .await
    .map_err(|_| Error::History)?
}

impl Session {
    fn selected_roots(
        &self,
        system: &SystemState,
        selected: &[Removal],
    ) -> Result<Vec<PathBuf>, Error> {
        let mut builds = selected
            .iter()
            .filter_map(|entry| match entry {
                Removal::Build { path, id } => Some((path, id)),
                Removal::Generation(_) => None,
            })
            .peekable();
        if builds.peek().is_none() {
            return Ok(Vec::new());
        }
        if !system.complete {
            return Err(Error::TargetUnavailable);
        }
        let registered = registered_roots(&self.auto_roots).map_err(history_error)?;
        let mut roots = Vec::new();
        let mut seen = HashSet::new();
        for (path, id) in builds {
            if !seen.insert(id)
                || system
                    .targets
                    .get(path)
                    .is_some_and(|target| target.protects_builds())
                || !self
                    .history
                    .builds
                    .iter()
                    .any(|build| &build.id == id && &build.store_path == path)
            {
                return Err(Error::TargetUnavailable);
            }
            let root = self.directory.join(result_link(id).map_err(history_error)?);
            let directory = root.parent().expect("a build root has a directory");
            // Only unlink entries inside the application's real build directories.
            if fs::canonicalize(directory).as_deref().ok() != Some(directory)
                || resolve_root(&root, &registered)
                    .map_err(history_error)?
                    .as_ref()
                    != Some(path)
            {
                return Err(Error::TargetUnavailable);
            }
            for entry in fs::read_dir(directory).map_err(history_error)? {
                if entry.map_err(history_error)?.file_name() != "result" {
                    return Err(history_error(io::Error::new(
                        io::ErrorKind::DirectoryNotEmpty,
                        format!("unexpected entries in {}", directory.display()),
                    )));
                }
            }
            roots.push(root);
        }
        Ok(roots)
    }
}

fn remove_roots(roots: Vec<PathBuf>) -> Result<(), Error> {
    for root in roots {
        fs::remove_file(&root).map_err(history_error)?;
        fs::remove_dir(root.parent().expect("a build root has a directory"))
            .map_err(history_error)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;
    use crate::rebuild::history::{Source, SystemTarget};

    #[test]
    fn app_deletion_removes_only_the_selected_builds_for_a_store_path() -> io::Result<()> {
        let temp = tempfile::tempdir()?;
        let directory = temp.path();
        let state = directory.join("state");
        let auto_roots = directory.join("auto");
        let output = directory.join("store-output");
        fs::create_dir(&auto_roots)?;
        fs::create_dir(&output)?;
        let mut session = Session::open_in(&state, &auto_roots)?;
        let mut target = Target::new(&output);
        for name in ["one", "two"] {
            let root = session.start_build(Source::Built {
                branch: None,
                revision: None,
            })?;
            symlink(&output, &root)?;
            symlink(&root, auto_roots.join(name))?;
            target.builds.push(session.finish_build(&output)?);
        }

        let profile = directory.join("system");
        let generation = directory.join("system-40-link");
        let current = directory.join("current-system");
        let booted = directory.join("booted-system");
        symlink(&output, &generation)?;
        let system = || {
            SystemState::discover(&profile, &current, &booted, |path| {
                fs::canonicalize(path).ok()
            })
        };
        let first = target.build_removal(&target.builds[0]).unwrap();
        let second = target.build_removal(&target.builds[1]).unwrap();
        let wrong_path = Removal::Build {
            path: directory.join("other-output"),
            id: target.builds[0].id.clone(),
        };
        assert!(matches!(
            session.selected_roots(&system(), &[wrong_path]),
            Err(Error::TargetUnavailable)
        ));
        assert!(matches!(
            session.selected_roots(&system(), &[first.clone(), first.clone()]),
            Err(Error::TargetUnavailable)
        ));
        symlink(&output, &current)?;
        assert!(matches!(
            session.selected_roots(&system(), std::slice::from_ref(&first)),
            Err(Error::TargetUnavailable)
        ));
        target.system = SystemTarget {
            active: true,
            ..SystemTarget::default()
        };
        assert!(target.build_removal(&target.builds[0]).is_none());
        fs::remove_file(&current)?;

        let first_root = state.join(&target.builds[0].gc_root);
        let second_root = state.join(&target.builds[1].gc_root);
        let unexpected = second_root.with_file_name("extra");
        fs::write(&unexpected, b"keep")?;
        assert!(matches!(
            session.selected_roots(&system(), &[first.clone(), second.clone()]),
            Err(Error::History)
        ));
        assert!(first_root.is_symlink() && second_root.is_symlink() && unexpected.is_file());
        fs::remove_file(&unexpected)?;

        let roots = session
            .selected_roots(&system(), std::slice::from_ref(&first))
            .map_err(|_| io::Error::other("expected the first registered root"))?;
        assert_eq!(roots, [first_root.clone()]);
        remove_roots(roots).map_err(|_| io::Error::other("removing the first root failed"))?;
        assert!(!first_root.parent().unwrap().exists());
        assert!(second_root.is_symlink());
        assert!(generation.is_symlink() && output.is_dir());
        assert!(auto_roots.join("one").is_symlink() && auto_roots.join("two").is_symlink());
        drop(session);

        let session = Session::open_in(&state, &auto_roots)?;
        assert_eq!(session.history.builds.len(), 1);
        assert_eq!(session.history.builds[0].id, target.builds[1].id);
        assert!(matches!(
            session.selected_roots(&system(), &[first]),
            Err(Error::TargetUnavailable)
        ));
        let roots = session
            .selected_roots(&system(), &[second])
            .map_err(|_| io::Error::other("expected the second registered root"))?;
        assert_eq!(roots, [second_root]);
        Ok(())
    }
}
