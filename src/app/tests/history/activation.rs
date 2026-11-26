use iced::{
    Event, Point, Rectangle, Size, Vector,
    advanced::{Layout, Shell, clipboard, mouse, subscription},
    event,
    futures::StreamExt,
    keyboard, window,
};
use nixos_manager_apply_protocol::system::Target as SystemTarget;

use crate::{
    app::{App, Message},
    i18n::LanguagePreference,
    rebuild::{self, history},
    ui::{
        icon,
        screens::{Screen, build_history},
    },
};

use super::super::support::{boot_with_cache, headless, layout};

#[tokio::test]
async fn escape_returns_from_activation_confirmation_to_build_history() {
    let mut app = boot_with_cache(Ok(None));
    let renderer = headless().await;
    let size = Size::new(800.0, 600.0);
    let escape_messages = async |app: &App, repeat| {
        let key = keyboard::Key::Named(keyboard::key::Named::Escape);
        let interaction = subscription::Event::Interaction {
            window: window::Id::unique(),
            event: Event::Keyboard(keyboard::Event::KeyPressed {
                key: key.clone(),
                modified_key: key,
                physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Escape),
                location: keyboard::Location::Standard,
                modifiers: keyboard::Modifiers::default(),
                text: None,
                repeat,
            }),
            status: event::Status::Ignored,
        };
        iced::futures::stream::select_all(
            subscription::into_recipes(app.subscription())
                .into_iter()
                .map(|recipe| {
                    recipe.stream(Box::pin(iced::futures::stream::iter([interaction.clone()])))
                }),
        )
        .collect::<Vec<_>>()
        .await
    };

    let path = "/nix/store/inactive-system";
    for target in [
        rebuild::ReadyTarget::Selected(path.into()),
        rebuild::ReadyTarget::Quick(history::QuickRollback::for_test(path)),
    ] {
        let mut state = build_history::State::default();
        let _ = state.update(build_history::Message::Select(target), None, true, true);
        app.screen = Screen::BuildHistory(state);
        let labels = layout(&mut app.view(), &renderer, size).2;
        assert!(labels.contains("Activate this configuration?"));

        let Ok([message]) = <[Message; 1]>::try_from(escape_messages(&app, false).await) else {
            panic!("expected one Escape message from the confirmation");
        };
        let _ = app.update(message);
        assert!(matches!(app.screen, Screen::BuildHistory(_)));
        let labels = layout(&mut app.view(), &renderer, size).2;
        assert!(labels.contains("Build History"));
        assert!(!labels.contains("Activate this configuration?"));
        assert!(escape_messages(&app, true).await.is_empty());

        let Ok([message]) = <[Message; 1]>::try_from(escape_messages(&app, false).await) else {
            panic!("expected one Escape message from the list");
        };
        let _ = app.update(message);
        assert!(matches!(app.screen, Screen::Overview));
    }
}

#[tokio::test]
async fn grouped_builds_use_the_same_activation_warning_as_system_generations() {
    let app = boot_with_cache(Ok(None));
    let renderer = headless().await;
    let size = Size::new(800.0, 600.0);
    let path = "/nix/store/older-system";
    let build: history::Build = serde_json::from_value(serde_json::json!({
        "id": "older",
        "gc_root": "builds/older/result",
        "store_path": path,
        "source": { "kind": "built", "branch": "main", "revision": "0123456789abcdef" },
    }))
    .expect("build record");
    let mut snapshot = history::Snapshot::default();
    snapshot.targets.push(history::Target {
        store_path: path.into(),
        builds: Vec::new(),
        system: SystemTarget {
            generations: vec![1],
            ..SystemTarget::default()
        },
    });
    let mut switched = snapshot
        .clone()
        .with_switch_boot_target_for_test("/nix/store/system-profile");
    let warning = app.i18n.text("history-activation-warning");
    let switch_warning = app.i18n.text("history-switch-non-system-profile-warning");

    for (snapshot, warns_about_switch) in [(&mut snapshot, false), (&mut switched, true)] {
        for builds in [Vec::new(), vec![build.clone()]] {
            snapshot.targets[0].builds = builds;
            for booted in [true, false] {
                snapshot.targets[0].system.booted = booted;
                let mut state = build_history::State::default();
                state.set_snapshot(&app.i18n, snapshot);
                let _ = state.update(
                    build_history::Message::Select(rebuild::ReadyTarget::Selected(path.into())),
                    Some(snapshot),
                    true,
                    true,
                );
                let mut view = state
                    .view(&app.i18n, Some(snapshot), true, None, None)
                    .map(Message::BuildHistory);
                let labels = layout(&mut view, &renderer, size).2;
                assert!(labels.contains("Activate this configuration?"));
                assert_eq!(labels.contains(&warning), !booted && !warns_about_switch);
                assert_eq!(
                    labels.contains(&switch_warning),
                    !booted && warns_about_switch
                );
            }
        }
    }
}

#[tokio::test]
async fn build_history_confirmations_show_activation_and_rollback_details() {
    let mut app = boot_with_cache(Ok(None));
    app.set_language(LanguagePreference::Russian);
    let mut state = build_history::State::default();
    let mut snapshot = history::Snapshot::default();
    let inactive_path = "/nix/store/inactive-system";
    snapshot.targets.push(history::Target {
        store_path: "/nix/store/active-system".into(),
        builds: Vec::new(),
        system: SystemTarget {
            generations: vec![2],
            active: true,
            ..SystemTarget::default()
        },
    });
    snapshot.targets.push(history::Target {
        store_path: inactive_path.into(),
        builds: ["main", "stable"]
            .map(|branch| {
                serde_json::from_value(serde_json::json!({
                    "id": branch,
                    "gc_root": format!("builds/{branch}/result"),
                    "store_path": inactive_path,
                    "source": { "kind": "built", "branch": branch, "revision": "0123456789abcdef" },
                    "built_at": "2026-01-02T03:04:05Z"
                }))
                .expect("build record")
            })
            .into(),
        system: SystemTarget {
            generations: vec![1, 0],
            selected_generation: Some(1),
            profile_selected: true,
            booted: true,
            ..SystemTarget::default()
        },
    });
    state.set_snapshot(&app.i18n, &snapshot);

    let renderer = headless().await;
    let size = Size::new(560.0, 540.0);
    let compact = Size::new(560.0 / 1.5, 420.0 / 1.5);
    let viewport = Rectangle::with_size(size);
    let warning = app.i18n.text("history-system-profile-activation-warning");
    let confirmation_labels = |state: &build_history::State, snapshot: &history::Snapshot| {
        layout(
            &mut state
                .view(&app.i18n, Some(snapshot), true, None, None)
                .map(Message::BuildHistory),
            &renderer,
            compact,
        )
        .2
    };
    let messages = {
        let mut view = state
            .view(&app.i18n, Some(&snapshot), true, None, None)
            .map(Message::BuildHistory);
        let (mut tree, node, labels) = layout(&mut view, &renderer, size);
        let icons: Vec<_> = labels
            .text
            .iter()
            .filter(|(text, _)| text == icon::glyph("play"))
            .map(|(_, bounds)| *bounds)
            .collect();
        assert_eq!(icons.len(), 1);
        assert!(!labels.contains("Активировать"));
        let icon = icons[0];
        let button = labels
            .containers
            .iter()
            .find(|bounds| bounds.size() == Size::new(34.0, 34.0) && bounds.contains(icon.center()))
            .expect("activation button");
        assert!((icon.center_x() - button.center_x()).abs() < 0.5);
        let [_, _, path, generation, ..] = labels.focusables.as_slice() else {
            panic!("expected paths and generation labels");
        };
        assert!((button.center_y() - path.center_y()).abs() < 0.5);
        assert!(button.y + button.height <= generation.y);
        let layout = Layout::new(&node);
        let widget = view.as_widget_mut();
        let cursor = mouse::Cursor::Available(icon.center());
        let mut messages = Vec::new();
        for event in [
            mouse::Event::CursorMoved {
                position: icon.center(),
            },
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ] {
            widget.update(
                &mut tree,
                &Event::Mouse(event),
                layout,
                cursor,
                &renderer,
                &mut clipboard::Null,
                &mut Shell::new(&mut messages),
                &viewport,
            );
            if matches!(event, mouse::Event::CursorMoved { .. }) {
                assert!(
                    widget
                        .overlay(&mut tree, layout, &renderer, &viewport, Vector::ZERO)
                        .is_some()
                );
            }
        }
        messages
    };
    let [
        Message::BuildHistory(
            message @ build_history::Message::Select(rebuild::ReadyTarget::Selected(path)),
        ),
    ] = messages.as_slice()
    else {
        panic!("expected an activation selection");
    };
    assert_eq!(path, std::path::Path::new(inactive_path));
    let _ = state.update(message.clone(), Some(&snapshot), true, true);

    let labels = confirmation_labels(&state, &snapshot);
    assert_eq!(labels.focusables.len(), 5);
    assert!(labels.contains("загружена"));
    assert!(!labels.contains(&warning));
    let active_badge = app.i18n.text("history-active");
    assert!(!labels.text.iter().any(|(label, _)| label == &active_badge));
    assert!(labels.contains("системный профиль"));
    let activate = labels.bounds("Активировать");
    assert!(activate.y + activate.height <= compact.height);
    let _ = state.update(build_history::Message::Dismiss, Some(&snapshot), true, true);

    let rollback = history::QuickRollback::for_test(inactive_path);
    snapshot.quick_rollback = Some(rollback.clone());
    let messages = {
        let mut view = state
            .view(&app.i18n, Some(&snapshot), true, None, None)
            .map(Message::BuildHistory);
        let (mut tree, node, labels) = layout(&mut view, &renderer, size);
        let position = labels.bounds("Откатить").center();
        let mut messages = Vec::new();
        for event in [
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ] {
            view.as_widget_mut().update(
                &mut tree,
                &Event::Mouse(event),
                Layout::new(&node),
                mouse::Cursor::Available(position),
                &renderer,
                &mut clipboard::Null,
                &mut Shell::new(&mut messages),
                &viewport,
            );
        }
        messages
    };
    for message in messages {
        let Message::BuildHistory(message) = message else {
            panic!("expected a build history message");
        };
        let _ = state.update(message, Some(&snapshot), true, true);
    }
    let labels = confirmation_labels(&state, &snapshot);
    assert_eq!(labels.focusables.len(), 5); // Path, two generations, and two builds.
    assert!(labels.contains("загружена"));
    assert!(!labels.contains(&warning));
    assert!(labels.contains("системный профиль"));
    let activate = labels.bounds("Активировать");
    assert!(activate.y + activate.height <= compact.height);
    assert!(matches!(
        state.update(build_history::Message::Activate, Some(&snapshot), true, true),
        build_history::Action::Activate(rebuild::ReadyTarget::Quick(target)) if target == rollback
    ));

    snapshot.targets[1].system.booted = false;
    state.set_snapshot(&app.i18n, &snapshot);
    let removal = snapshot.targets[1].generation_removal(0).unwrap();
    let _ = state.update(
        build_history::Message::ToggleRemoval(removal),
        Some(&snapshot),
        true,
        true,
    );
    let _ = state.update(
        build_history::Message::Select(rebuild::ReadyTarget::Quick(rollback)),
        Some(&snapshot),
        true,
        true,
    );
    let labels = confirmation_labels(&state, &snapshot);
    assert!(labels.contains(&warning));
    assert!(!labels.contains(&app.i18n.text("history-activation-warning")));

    let messages = {
        let mut view = state
            .view(&app.i18n, Some(&snapshot), true, None, None)
            .map(Message::BuildHistory);
        let (mut tree, node, _) = layout(&mut view, &renderer, size);
        let mut messages = Vec::new();
        view.as_widget_mut().update(
            &mut tree,
            &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Layout::new(&node),
            mouse::Cursor::Available(Point::new(size.width / 2.0, 4.0)),
            &renderer,
            &mut clipboard::Null,
            &mut Shell::new(&mut messages),
            &viewport,
        );
        messages
    };
    let [Message::BuildHistory(message @ build_history::Message::Dismiss)] = messages.as_slice()
    else {
        panic!("expected the backdrop click to dismiss the confirmation");
    };
    assert!(matches!(
        state.update(message.clone(), Some(&snapshot), true, true),
        build_history::Action::None
    ));
    let labels = confirmation_labels(&state, &snapshot);
    assert!(labels.contains(&app.i18n.text("history-title")));
    assert!(labels.contains(&app.i18n.text("history-removal-boot-warning")));
}

#[tokio::test]
async fn build_history_shows_a_disabled_rollback_button_with_a_tooltip() {
    let mut app = boot_with_cache(Ok(None));
    app.set_language(LanguagePreference::Russian);
    let renderer = headless().await;
    let size = Size::new(560.0 / 1.5, 420.0 / 1.5);
    let viewport = Rectangle::with_size(size);
    let state = build_history::State::default();
    let empty = history::Snapshot::default();
    let mut unavailable = history::Snapshot::default();
    unavailable.unavailable = true;

    for snapshot in [None, Some(&unavailable), Some(&empty)] {
        let can_activate = snapshot.is_some_and(|snapshot| !snapshot.unavailable);
        let mut view = state
            .view(&app.i18n, snapshot, can_activate, None, None)
            .map(Message::BuildHistory);
        let (mut tree, node, labels) = layout(&mut view, &renderer, size);
        let button = labels.bounds("Откатить");
        let position = button.center();
        assert!(viewport.contains(position));
        let mut messages = Vec::new();
        let widget = view.as_widget_mut();
        for event in [
            mouse::Event::CursorMoved { position },
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ] {
            widget.update(
                &mut tree,
                &Event::Mouse(event),
                Layout::new(&node),
                mouse::Cursor::Available(position),
                &renderer,
                &mut clipboard::Null,
                &mut Shell::new(&mut messages),
                &viewport,
            );
        }
        assert!(!messages.iter().any(|message| matches!(
            message,
            Message::BuildHistory(build_history::Message::Select(_))
        )));
        let mut tooltip = widget
            .overlay(
                &mut tree,
                Layout::new(&node),
                &renderer,
                &viewport,
                Vector::ZERO,
            )
            .expect("disabled rollback tooltip");
        let node = tooltip.as_overlay_mut().layout(&renderer, size);
        let mut label_layout = Layout::new(&node);
        while let Some(child) = label_layout.children().next() {
            label_layout = child;
        }
        let hint = label_layout.bounds();
        assert!(hint.width > 0.0 && hint.y + hint.height <= button.y);
    }
}
