use iced::{
    Event, Point, Rectangle, Size, Vector,
    advanced::{Layout, Shell, clipboard, mouse},
};
use nixos_manager_apply_protocol::system::Target as SystemTarget;

use crate::{
    app::Message,
    i18n::LanguagePreference,
    rebuild::{self, history},
    ui::{icon, screens::build_history},
};

use super::super::support::{boot_with_cache, headless, layout};

#[tokio::test]
async fn build_history_marks_system_generations_and_app_builds_independently() {
    let app = boot_with_cache(Ok(None));
    let path = "/nix/store/shared-system";
    let builds = ["gen-one", "gen-two"].map(|id| {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "gc_root": format!("builds/{id}/result"),
            "store_path": path,
            "source": { "kind": "built", "branch": id, "revision": "0123456789abcdef" }
        }))
        .expect("build record")
    });
    let mut snapshot = history::Snapshot::default();
    snapshot.targets.push(history::Target {
        store_path: path.into(),
        builds: builds.into(),
        system: SystemTarget {
            generations: vec![42],
            ..SystemTarget::default()
        },
    });
    let mut state = build_history::State::default();
    state.set_snapshot(&app.i18n, &snapshot);
    let renderer = headless().await;
    let size = Size::new(560.0, 540.0);
    let viewport = Rectangle::with_size(size);

    for index in [0, 1, 2, 1] {
        let messages = {
            let mut view = state
                .view(&app.i18n, Some(&snapshot), true, None, None)
                .map(Message::BuildHistory);
            let (mut tree, node, labels) = layout(&mut view, &renderer, size);
            let trash: Vec<_> = labels
                .text
                .iter()
                .filter(|(label, _)| label == icon::glyph("trash"))
                .map(|(_, bounds)| bounds.center())
                .collect();
            assert_eq!(trash.len(), 3);
            assert_eq!(labels.focusables.len(), 4);
            let activation = labels.bounds(icon::glyph("play")).center();
            let activation_width = labels
                .containers
                .iter()
                .filter(|bounds| bounds.contains(activation))
                .map(|bounds| bounds.width)
                .min_by(f32::total_cmp)
                .expect("activation button");
            let inset = labels.focusables[1].x - labels.focusables[0].x;
            assert!(inset > 0.0);
            assert!(
                trash
                    .iter()
                    .zip(&labels.focusables[1..])
                    .all(|(icon, label)| {
                        (icon.y - label.center_y()).abs() < 1.0
                            && (activation.x - icon.x - inset - activation_width).abs() < 1.0
                    })
            );
            let mut messages = Vec::new();
            for event in [
                mouse::Event::ButtonPressed(mouse::Button::Left),
                mouse::Event::ButtonReleased(mouse::Button::Left),
            ] {
                view.as_widget_mut().update(
                    &mut tree,
                    &Event::Mouse(event),
                    Layout::new(&node),
                    mouse::Cursor::Available(trash[index]),
                    &renderer,
                    &mut clipboard::Null,
                    &mut Shell::new(&mut messages),
                    &viewport,
                );
            }
            messages
        };
        let [Message::BuildHistory(message @ build_history::Message::ToggleRemoval(_))] =
            messages.as_slice()
        else {
            panic!("expected a removal toggle for entry {index}");
        };
        let _ = state.update(message.clone(), Some(&snapshot), true, true);
    }

    let build_history::Action::Remove(marked) =
        state.update(build_history::Message::Remove, Some(&snapshot), true, true)
    else {
        panic!("expected selected system and app entries");
    };
    assert!(matches!(
        marked.as_slice(),
        [history::Removal::Generation(generation), history::Removal::Build { path: build_path, id }]
            if generation.number == 42 && generation.path == std::path::Path::new(path)
                && build_path == std::path::Path::new(path) && id == "gen-two"
    ));
}

#[tokio::test]
async fn build_history_toggles_removal_for_an_old_duplicate_generation() {
    #[derive(Clone, Copy)]
    enum Control {
        Trash,
        Remove,
        Clear,
        Refresh,
        Backdrop,
        Close,
    }

    let mut app = boot_with_cache(Ok(None));
    app.set_language(LanguagePreference::Russian);
    let mut snapshot = history::Snapshot::default();
    snapshot.targets.push(history::Target {
        store_path: "/nix/store/active-system".into(),
        builds: Vec::new(),
        system: SystemTarget {
            generations: vec![42, 40],
            profile_selected: true,
            selected_generation: Some(42),
            active: true,
            booted: true,
        },
    });
    let mut state = build_history::State::default();
    state.set_snapshot(&app.i18n, &snapshot);
    let renderer = headless().await;
    let size = Size::new(560.0, 420.0);
    let interact = |state: &build_history::State, size: Size, control, marked| {
        let viewport = Rectangle::with_size(size);
        let mut view = state
            .view(&app.i18n, Some(&snapshot), false, None, None)
            .map(Message::BuildHistory);
        let (mut tree, node, labels) = layout(&mut view, &renderer, size);
        let remove = labels.bounds("Удалить выбранные");
        let refresh = labels.bounds(icon::glyph("refresh"));
        assert!(remove.x < refresh.x);
        assert!(remove.y + remove.height <= size.height);
        assert!(refresh.y + refresh.height <= size.height);
        assert_eq!(
            labels.contains(&app.i18n.text("history-removal-boot-warning")),
            marked
        );
        let close = labels.bounds(icon::glyph("close")).center();
        let clear = labels
            .containers
            .iter()
            .copied()
            .find(|bounds| {
                bounds.size() == Size::new(34.0, 34.0)
                    && bounds.x >= remove.x + remove.width
                    && bounds.x + bounds.width <= refresh.x
                    && (bounds.center_y() - refresh.center_y()).abs() < 1.0
            })
            .unwrap_or_else(|| {
                panic!(
                    "expected clear-selection button between {remove:?} and {refresh:?} at {size:?}, found {:?}",
                    labels.containers
                )
            });
        let mut controls: Vec<_> = labels
            .containers
            .into_iter()
            .filter(|bounds| {
                bounds.size() == Size::new(34.0, 34.0)
                    && *bounds != clear
                    && !bounds.contains(close)
                    && !bounds.contains(refresh.center())
            })
            .collect();
        controls.dedup();
        let [trash] = controls.as_slice() else {
            panic!("expected one trash button, found {controls:?}")
        };
        let position = match control {
            Control::Trash => trash.center(),
            Control::Remove => remove.center(),
            Control::Clear => clear.center(),
            Control::Refresh => refresh.center(),
            Control::Backdrop => Point::new(size.width / 2.0, 4.0),
            Control::Close => close,
        };
        let layout = Layout::new(&node);
        let widget = view.as_widget_mut();
        let mut messages = Vec::new();
        for event in [
            mouse::Event::CursorMoved { position },
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ] {
            widget.update(
                &mut tree,
                &Event::Mouse(event),
                layout,
                mouse::Cursor::Available(position),
                &renderer,
                &mut clipboard::Null,
                &mut Shell::new(&mut messages),
                &viewport,
            );
            if matches!(
                (control, event),
                (
                    Control::Trash | Control::Clear | Control::Refresh,
                    mouse::Event::CursorMoved { .. }
                )
            ) {
                assert!(
                    widget
                        .overlay(&mut tree, layout, &renderer, &viewport, Vector::ZERO)
                        .is_some()
                );
            }
        }
        messages
    };
    assert!(matches!(
        interact(&state, size, Control::Refresh, false).as_slice(),
        [Message::BuildHistory(build_history::Message::Refresh)]
    ));
    assert!(interact(&state, size, Control::Remove, false).is_empty());
    assert!(interact(&state, size, Control::Clear, false).is_empty());
    for marked in [false, true, false] {
        let messages = interact(&state, size, Control::Backdrop, marked);
        if marked {
            assert!(messages.is_empty());
        } else {
            assert!(matches!(
                messages.as_slice(),
                [Message::BuildHistory(build_history::Message::Dismiss)]
            ));
        }
        let messages = interact(&state, size, Control::Trash, marked);
        let [Message::BuildHistory(message @ build_history::Message::ToggleRemoval(removal))] =
            messages.as_slice()
        else {
            panic!("expected a removal toggle");
        };
        assert_eq!(removal.generation().unwrap().number, 40);
        let _ = state.update(message.clone(), Some(&snapshot), false, true);
    }
    assert!(matches!(
        state.update(
            build_history::Message::Dismiss,
            Some(&snapshot),
            false,
            true
        ),
        build_history::Action::None
    ));
    assert!(matches!(
        interact(&state, size, Control::Close, true).as_slice(),
        [Message::BuildHistory(build_history::Message::Close)]
    ));
    assert!(matches!(
        state.update(build_history::Message::Close, Some(&snapshot), false, true),
        build_history::Action::Close
    ));
    assert!(matches!(
        interact(&state, size, Control::Remove, true).as_slice(),
        [Message::BuildHistory(build_history::Message::Remove)]
    ));
    assert!(matches!(
        state.update(build_history::Message::Remove, Some(&snapshot), false, true),
        build_history::Action::Remove(marked) if marked.len() == 1 && marked[0].generation().unwrap().number == 40
    ));
    assert!(matches!(
        state.update(
            build_history::Message::Refresh,
            Some(&snapshot),
            false,
            true
        ),
        build_history::Action::None
    ));
    let _ = state.update(
        build_history::Message::ClearMarked,
        Some(&snapshot),
        false,
        true,
    );
    state.removal_finished(Err(rebuild::Error::Cancelled));
    state.set_snapshot(&app.i18n, &snapshot);
    let compact = Size::new(560.0 / 1.5, 170.0);
    assert!(matches!(
        interact(&state, compact, Control::Remove, true).as_slice(),
        [Message::BuildHistory(build_history::Message::Remove)]
    ));
    let messages = interact(&state, compact, Control::Clear, true);
    let [Message::BuildHistory(message @ build_history::Message::ClearMarked)] =
        messages.as_slice()
    else {
        panic!("expected a clear-selection message");
    };
    assert!(matches!(
        state.update(message.clone(), Some(&snapshot), false, true),
        build_history::Action::None
    ));
    assert!(interact(&state, compact, Control::Remove, false).is_empty());
    assert!(matches!(
        interact(&state, size, Control::Backdrop, false).as_slice(),
        [Message::BuildHistory(build_history::Message::Dismiss)]
    ));
    let messages = interact(&state, size, Control::Trash, false);
    let [Message::BuildHistory(message)] = messages.as_slice() else {
        panic!("expected a removal toggle");
    };
    let _ = state.update(message.clone(), Some(&snapshot), false, true);
    snapshot.targets[0].system.generations = vec![42];
    state.set_snapshot(&app.i18n, &snapshot);
    assert!(matches!(
        state.update(build_history::Message::Remove, Some(&snapshot), false, true),
        build_history::Action::None
    ));
}
