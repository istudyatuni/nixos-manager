use std::{io, path::PathBuf};

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
    Builds { path: PathBuf, ids: Vec<String> },
}

impl Removal {
    pub fn generation(&self) -> Option<&Generation> {
        match self {
            Self::Generation(generation) => Some(generation),
            Self::Builds { .. } => None,
        }
    }
}

impl Target {
    pub fn removal(&self, number: Option<u64>) -> Option<Removal> {
        match number {
            Some(number) if self.system.can_remove_generation(number, |_| false) => {
                Some(Removal::Generation(Generation {
                    number,
                    path: self.store_path.clone(),
                }))
            }
            None if !self.builds.is_empty() && !self.system.protects_builds() => {
                Some(Removal::Builds {
                    path: self.store_path.clone(),
                    ids: build_ids(self.builds.iter()),
                })
            }
            _ => None,
        }
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

fn build_ids<'a>(builds: impl Iterator<Item = &'a Build>) -> Vec<String> {
    let mut ids: Vec<_> = builds.map(|build| build.id.clone()).collect();
    ids.sort_unstable();
    ids.dedup();
    ids
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
                Removal::Builds { path, ids } => Some((path, ids)),
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
        for (path, ids) in builds {
            if ids.is_empty()
                || system
                    .targets
                    .get(path)
                    .is_some_and(|target| target.protects_builds())
                || build_ids(
                    self.history
                        .builds
                        .iter()
                        .filter(|build| &build.store_path == path),
                ) != *ids
            {
                return Err(Error::TargetUnavailable);
            }
            for id in ids {
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
    fn app_deletion_checks_the_entire_group_and_removes_its_build_directories() -> io::Result<()> {
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
        let selected = [target.removal(None).expect("application build group")];
        let first_only = Removal::Builds {
            path: output.clone(),
            ids: vec![target.builds[0].id.clone()],
        };
        assert!(matches!(
            session.selected_roots(&system(), &[first_only]),
            Err(Error::TargetUnavailable)
        ));
        symlink(&output, &current)?;
        assert!(matches!(
            session.selected_roots(&system(), &selected),
            Err(Error::TargetUnavailable)
        ));
        target.system = SystemTarget {
            active: true,
            ..SystemTarget::default()
        };
        assert!(target.removal(None).is_none());
        fs::remove_file(&current)?;

        let root = state.join(&target.builds[0].gc_root);
        let unexpected = root.with_file_name("extra");
        fs::write(&unexpected, b"keep")?;
        assert!(matches!(
            session.selected_roots(&system(), &selected),
            Err(Error::History)
        ));
        assert!(root.is_symlink() && unexpected.is_file());
        fs::remove_file(&unexpected)?;

        let roots = session
            .selected_roots(&system(), &selected)
            .map_err(|_| io::Error::other("expected both registered roots"))?;
        assert_eq!(roots.len(), 2);
        remove_roots(roots.clone()).map_err(|_| io::Error::other("removing build roots failed"))?;
        assert!(roots.iter().all(|root| !root.parent().unwrap().exists()));
        assert!(generation.is_symlink() && output.is_dir());
        assert!(auto_roots.join("one").is_symlink() && auto_roots.join("two").is_symlink());
        drop(session);

        let session = Session::open_in(&state, &auto_roots)?;
        assert!(session.history.builds.is_empty());
        Ok(())
    }
}
