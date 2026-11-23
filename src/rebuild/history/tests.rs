use std::os::unix::fs::symlink;

use super::*;

fn build_source(branch: &str, revision: &str) -> Source {
    Source::Built {
        branch: Some(branch.into()),
        revision: Some(revision.into()),
    }
}

#[test]
fn registered_roots_recover_and_retain_builds_individually() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let state = temp.path().join("state");
    let auto_roots = temp.path().join("auto");
    let store_path = temp.path().join("system-output");
    fs::create_dir(&auto_roots)?;
    fs::create_dir(&store_path)?;

    let mut session = Session::open_in(&state, &auto_roots)?;
    let first_root = session.start_build(build_source("release", "0123456780"))?;
    symlink(&store_path, &first_root)?;
    let first_registration = auto_roots.join("first");
    symlink(&first_root, &first_registration)?;
    let first = session.finish_build(&store_path)?;
    assert!(first.built_at.is_some());

    let pending_root = session.start_build(build_source("release", "0123456789"))?;
    let pending_registration = auto_roots.join("pending");
    symlink(&store_path, &pending_root)?;
    symlink(&pending_root, &pending_registration)?;
    assert!(
        matches!(Session::open_in(&state, &auto_roots), Err(error) if error.kind() == io::ErrorKind::WouldBlock)
    );
    drop(session);

    let session = Session::open_in(&state, &auto_roots)?;
    assert_eq!(session.history.builds.len(), 2);
    assert!(session.history.pending_build.is_none());
    let recovered = &session.history.builds[1];
    assert!(
        matches!(&recovered.source, Source::Built { branch: Some(branch), revision: Some(revision) }
        if branch == "release" && revision == "0123456789")
    );
    assert!(recovered.built_at.is_some());
    drop(session);

    fs::remove_file(first_registration)?;
    let session = Session::open_in(&state, &auto_roots)?;
    assert_eq!(session.history.builds.len(), 1);
    assert!(!session.cached(&first));
    assert!(first_root.is_symlink());
    drop(session);

    fs::remove_file(pending_root)?;
    let session = Session::open_in(&state, &auto_roots)?;
    assert!(session.history.builds.is_empty());
    assert!(pending_registration.is_symlink());
    assert!(store_path.is_dir());
    Ok(())
}

#[test]
fn rebuilding_a_commit_updates_only_the_latest_legacy_entry() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let state = temp.path().join("state");
    let auto_roots = temp.path().join("auto");
    let store_path = temp.path().join("system-output");
    fs::create_dir(&auto_roots)?;
    fs::create_dir(&store_path)?;

    let mut session = Session::open_in(&state, &auto_roots)?;
    let first_root = session.start_build(build_source("release", "0123456780"))?;
    symlink(&store_path, &first_root)?;
    symlink(&first_root, auto_roots.join("first"))?;
    let first = session.finish_build(&store_path)?;
    let latest_root = session.start_build(build_source("release", "0123456789"))?;
    assert_ne!(first_root, latest_root);
    symlink(&store_path, &latest_root)?;
    symlink(&latest_root, auto_roots.join("latest"))?;
    let latest = session.finish_build(&store_path)?;
    // Recreate history written before deduplication.
    session.history.builds[0].source = build_source("release", "0123456789");
    session.history.builds[1].built_at = Some(DateTime::UNIX_EPOCH);
    session.save()?;
    drop(session);

    let mut session = Session::open_in(&state, &auto_roots)?;
    assert_eq!(session.history.builds.len(), 2);
    let root = session.start_build(build_source("main", "0123456789"))?;
    assert_eq!(root, latest_root);
    let rebuilt = session.finish_build(&store_path)?;
    assert_eq!(rebuilt.id, latest.id);
    assert!(rebuilt.built_at > Some(DateTime::UNIX_EPOCH));
    drop(session);

    let session = Session::open_in(&state, &auto_roots)?;
    let [older, updated] = session.history.builds.as_slice() else {
        panic!("expected two entries from the legacy history");
    };
    assert_eq!(older.id, first.id);
    assert_eq!(older.built_at, first.built_at);
    assert_eq!(updated.id, latest.id);
    assert_eq!(updated.built_at, rebuilt.built_at);
    assert!(
        matches!(&updated.source, Source::Built { branch: Some(branch), .. } if branch == "main")
    );
    Ok(())
}

#[test]
fn interrupted_rebuild_recovers_only_when_the_result_changes() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let state = temp.path().join("state");
    let auto_roots = temp.path().join("auto");
    let original = temp.path().join("original-output");
    let replacement = temp.path().join("replacement-output");
    fs::create_dir(&auto_roots)?;
    fs::create_dir(&original)?;
    fs::create_dir(&replacement)?;

    let mut session = Session::open_in(&state, &auto_roots)?;
    let root = session.start_build(build_source("release", "0123456789"))?;
    symlink(&original, &root)?;
    symlink(&root, auto_roots.join("result"))?;
    let first = session.finish_build(&original)?;
    assert_eq!(
        session.start_build(build_source("main", "0123456789"))?,
        root
    );
    drop(session);

    let mut session = Session::open_in(&state, &auto_roots)?;
    assert_eq!(session.history.builds.len(), 1);
    assert!(session.cached(&first));
    assert_eq!(session.history.builds[0].built_at, first.built_at);
    assert!(
        matches!(&session.history.builds[0].source, Source::Built { branch: Some(branch), .. } if branch == "release")
    );
    assert!(session.history.pending_build.is_none());
    assert_eq!(
        session.start_build(build_source("main", "0123456789"))?,
        root
    );
    fs::remove_file(&root)?;
    symlink(&replacement, &root)?;
    drop(session);

    let mut session = Session::open_in(&state, &auto_roots)?;
    assert_eq!(session.history.builds.len(), 1);
    let recovered = &session.history.builds[0];
    assert_eq!(recovered.id, first.id);
    assert_eq!(recovered.store_path, replacement);
    assert!(
        matches!(&recovered.source, Source::Built { branch: Some(branch), .. } if branch == "main")
    );
    assert!(session.history.pending_build.is_none());

    fs::remove_file(&root)?;
    assert_ne!(
        session.start_build(build_source("main", "0123456789"))?,
        root
    );
    Ok(())
}

#[test]
fn interrupted_switch_keeps_the_confirmed_registration() -> io::Result<()> {
    let state = tempfile::tempdir()?;
    let auto_roots = state.path().join("auto");
    let previous = PathBuf::from("/nix/store/previous");
    let mut session = Session::open_in(state.path(), &auto_roots)?;
    session.start_operation(
        Mode::BootAndActivate,
        Path::new("/nix/store/next"),
        Some(previous.clone()),
    )?;
    session.registered()?;
    drop(session);

    let session = Session::open_in(state.path(), &auto_roots)?;
    let operation = &session.history.operations[0];
    assert!(operation.registered);
    assert!(operation.outcome == Outcome::Interrupted);
    assert_eq!(operation.previous_active.as_ref(), Some(&previous));
    Ok(())
}

#[test]
fn quick_rollback_uses_the_previous_active_path_after_test_or_switch() -> io::Result<()> {
    for mode in [Mode::Activate, Mode::BootAndActivate] {
        let state = tempfile::tempdir()?;
        let mut session = Session::open_in(state.path(), &state.path().join("auto"))?;
        let a = PathBuf::from("/nix/store/active-before-application");
        let b = PathBuf::from("/nix/store/built-but-unused");
        let c = PathBuf::from("/nix/store/applied");
        let d = PathBuf::from("/nix/store/boot-only");
        let mut targets = BTreeMap::new();
        for path in [&a, &b, &c, &d] {
            add_target(&mut targets, path);
        }
        session.start_operation(Mode::Boot, &d, Some(a.clone()))?;
        session.finish_operation(Outcome::Succeeded, Some(a.clone()))?;
        assert!(session.history.quick_rollback(Some(&a), &targets).is_none());

        session.start_operation(mode, &c, Some(a.clone()))?;
        session.finish_operation(Outcome::Succeeded, Some(c.clone()))?;
        session.start_operation(Mode::Boot, &d, Some(c.clone()))?;
        session.finish_operation(Outcome::Succeeded, Some(c.clone()))?;
        assert_eq!(
            session
                .history
                .quick_rollback(Some(&c), &targets)
                .unwrap()
                .target,
            a
        );
        assert!(session.history.quick_rollback(Some(&d), &targets).is_none());
        targets.remove(&a);
        assert!(session.history.quick_rollback(Some(&c), &targets).is_none());
    }
    Ok(())
}

#[test]
fn discovers_profile_generations_and_independent_live_statuses() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let directory = temp.path();
    let saved = directory.join("saved-output");
    let active = directory.join("active-output");
    fs::create_dir(&saved)?;
    fs::create_dir(&active)?;
    symlink(&saved, directory.join("system-2-link"))?;
    symlink(&saved, directory.join("system-5-link"))?;
    symlink("system-2-link", directory.join("system"))?;
    symlink("missing", directory.join("system-9-link"))?;
    symlink(&active, directory.join("current-system"))?;
    symlink(&saved, directory.join("booted-system"))?;

    let discover = |targets: &mut BTreeMap<PathBuf, Target>| {
        discover_system(
            &directory.join("system"),
            &directory.join("current-system"),
            &directory.join("booted-system"),
            targets,
            |path| fs::canonicalize(path).ok(),
        )
    };
    let mut targets = BTreeMap::new();
    assert_eq!(discover(&mut targets), Some(active.clone()));
    assert_eq!(targets.len(), 2);
    assert!(targets[&active].active);
    assert!(!targets[&active].booted);
    assert!(targets[&active].generations.is_empty());
    assert!(targets[&saved].booted);
    assert!(!targets[&saved].active);
    assert_eq!(targets[&saved].selected_generation, Some(2));
    assert_eq!(targets[&saved].generations, [5, 2]);

    fs::remove_file(directory.join("booted-system"))?;
    symlink(&active, directory.join("booted-system"))?;
    targets.clear();
    discover(&mut targets);
    assert!(targets[&active].active && targets[&active].booted);
    Ok(())
}

#[test]
fn uses_live_activation_even_if_switch_reported_an_error() -> io::Result<()> {
    let state = tempfile::tempdir()?;
    let mut session = Session::open_in(state.path(), &state.path().join("auto"))?;
    let a = PathBuf::from("/nix/store/previous");
    let b = PathBuf::from("/nix/store/next");
    let mut targets = BTreeMap::new();
    add_target(&mut targets, &a);
    add_target(&mut targets, &b);

    session.start_operation(Mode::BootAndActivate, &b, Some(a.clone()))?;
    session.registered()?;
    session.finish_operation(Outcome::Failed, Some(a.clone()))?;
    assert!(session.history.quick_rollback(Some(&a), &targets).is_none());

    session.start_operation(Mode::BootAndActivate, &b, Some(a.clone()))?;
    session.registered()?;
    session.finish_operation(Outcome::Failed, Some(b.clone()))?;
    assert_eq!(
        session
            .history
            .quick_rollback(Some(&b), &targets)
            .unwrap()
            .target,
        a
    );
    Ok(())
}
