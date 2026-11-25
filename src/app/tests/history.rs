use iced::{
    Event, Point, Rectangle, Size, Vector,
    advanced::{Layout, Shell, clipboard, layout, mouse, overlay},
    keyboard,
};
use nixos_manager_apply_protocol::system::Target as SystemTarget;

use crate::{
    app::Message,
    i18n::LanguagePreference,
    rebuild::history,
    ui::{
        icon,
        screens::{Screen, build_history, overview},
    },
};

use super::support::{Labels, boot_with_cache, headless, layout};

mod activation;
mod removal;

#[tokio::test]
async fn build_history_shows_system_configurations_without_a_repository() {
    use crate::ui::style::spacing;

    let mut app = boot_with_cache(Ok(None));
    let renderer = headless().await;
    let size = Size::new(800.0, 540.0);
    let viewport = Rectangle::with_size(size);
    let click = [
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
    ];
    let (trigger, mut messages) = {
        let mut view = app.view();
        let (mut tree, node, labels) = layout(&mut view, &renderer, size);
        assert!(!labels.contains("Build History"));
        let trigger = labels.bounds(icon::glyph("menu"));
        let mut messages = Vec::new();
        for event in &click {
            view.as_widget_mut().update(
                &mut tree,
                event,
                Layout::new(&node),
                mouse::Cursor::Available(trigger.center()),
                &renderer,
                &mut clipboard::Null,
                &mut Shell::new(&mut messages),
                &viewport,
            );
        }
        (trigger, messages)
    };
    assert!(matches!(
        messages.as_slice(),
        [Message::Overview(overview::Message::OpenActionsMenu)]
    ));
    let _ = app.update(messages.pop().unwrap());
    {
        let mut view = app.view();
        let (mut tree, node, _) = layout(&mut view, &renderer, size);
        let overlay = view
            .as_widget_mut()
            .overlay(
                &mut tree,
                Layout::new(&node),
                &renderer,
                &viewport,
                Vector::ZERO,
            )
            .expect("actions menu");
        let mut overlay = overlay::Nested::new(overlay);
        let node = overlay.layout(&renderer, size);
        let layout = Layout::new(&node);
        let mut labels = Labels::default();
        overlay.operate(layout, &renderer, &mut labels);
        let item = labels.bounds("Build History");
        assert!(item.y >= trigger.y + trigger.height);
        for event in &click {
            overlay.update(
                event,
                layout,
                mouse::Cursor::Available(item.center()),
                &renderer,
                &mut clipboard::Null,
                &mut Shell::new(&mut messages),
            );
        }
    }
    assert!(matches!(
        messages.as_slice(),
        [Message::Overview(overview::Message::OpenBuildHistory)]
    ));
    let _ = app.update(messages.pop().unwrap());
    assert!(!app.overview.actions_menu_open);
    assert!(matches!(app.screen, Screen::BuildHistory(_)));
    let mut snapshot = history::Snapshot::default();
    snapshot
        .targets
        .extend((42..57).map(|number| history::Target {
            store_path: format!("/nix/store/example-{number}").into(),
            builds: Vec::new(),
            system: SystemTarget {
                generations: vec![number],
                profile_selected: number == 42,
                selected_generation: (number == 42).then_some(number),
                active: number == 42,
                booted: number == 42,
            },
        }));
    let _ = app.update(Message::HistoryLoaded(app.history_generation, snapshot));

    app.set_language(LanguagePreference::Russian);
    let compact = Size::new(560.0 / 1.5, 420.0 / 1.5);
    let mut view = app.view();
    let labels = layout(&mut view, &renderer, compact).2;
    let (viewport, _) = labels.scroll_viewport.expect("scrollable build history");
    let last_generation = labels.focusables.last().expect("selectable generation");
    assert!(last_generation.y > viewport.y + viewport.height);
    let activate = labels.bounds(icon::glyph("play"));
    let activate_button = labels
        .containers
        .iter()
        .find(|bounds| bounds.size() == Size::new(34.0, 34.0) && bounds.contains(activate.center()))
        .expect("activation button");
    let scrollbar_width = 10.0;
    assert!(
        activate_button.x + activate_button.width <= viewport.x + viewport.width - scrollbar_width
    );
    let title = labels.bounds("История сборок");
    let close = labels.bounds(icon::glyph("close"));
    assert!(close.x >= title.x + title.width);
    assert!(close.y + close.height <= viewport.y);
    let refresh = labels.bounds(icon::glyph("refresh"));
    assert!(
        refresh.y >= viewport.y + viewport.height && refresh.y + refresh.height <= compact.height
    );
    let help_icon = |labels: &Labels, caption| {
        let caption = labels.bounds(caption);
        labels
            .text
            .iter()
            .find_map(|(label, bounds)| {
                (label == icon::glyph("help")
                    && bounds.x >= caption.x + caption.width
                    && (caption.y..=caption.y + caption.height).contains(&bounds.center_y()))
                .then_some(*bounds)
            })
            .expect("help icon to the right of the caption")
    };
    let profile = labels.bounds("системный профиль");
    help_icon(&labels, "активна");
    help_icon(&labels, "системный профиль");
    help_icon(&labels, "загружена");
    assert!(profile.y + profile.height <= labels.focusables[1].y);

    let (mut tree, node, labels) = layout(&mut view, &renderer, size);
    let widget = view.as_widget_mut();
    let layout = Layout::new(&node);
    let window = Rectangle::with_size(size);
    for (caption, help_key) in [
        ("активна", "history-active-help"),
        ("загружена", "history-booted-help"),
        ("системный профиль", "history-system-profile-help"),
    ] {
        let icon = help_icon(&labels, caption);
        assert!(
            widget
                .overlay(&mut tree, layout, &renderer, &window, Vector::ZERO)
                .is_none()
        );
        widget.update(
            &mut tree,
            &Event::Mouse(mouse::Event::CursorMoved {
                position: icon.center(),
            }),
            layout,
            mouse::Cursor::Available(icon.center()),
            &renderer,
            &mut clipboard::Null,
            &mut Shell::new(&mut Vec::new()),
            &window,
        );
        let tooltip_text = {
            let tip = widget
                .overlay(&mut tree, layout, &renderer, &window, Vector::ZERO)
                .expect("status tooltip on hover");
            let mut tip = overlay::Nested::new(tip);
            let node = tip.layout(&renderer, size);
            let mut tooltip_labels = Labels::default();
            tip.operate(Layout::new(&node), &renderer, &mut tooltip_labels);
            tooltip_labels.bounds(&app.i18n.text(help_key))
        };
        let gap_offset = spacing::MD + spacing::SM / 2.0;
        let gap_y = if tooltip_text.center_y() > icon.center_y() {
            tooltip_text.y - gap_offset
        } else {
            tooltip_text.y + tooltip_text.height + gap_offset
        };
        let gap = Point::new(icon.center_x(), gap_y);
        assert!(
            !icon.contains(gap) && !tooltip_text.contains(gap),
            "icon {icon:?}, tooltip {tooltip_text:?}, gap {gap:?}"
        );
        for (point, stays_open) in [
            (gap, true),
            (tooltip_text.center(), true),
            (Point::ORIGIN, false),
        ] {
            let cursor = mouse::Cursor::Available(point);
            let event = Event::Mouse(mouse::Event::CursorMoved { position: point });
            let underlying_cursor = {
                let tip = widget
                    .overlay(&mut tree, layout, &renderer, &window, Vector::ZERO)
                    .expect("tooltip stays open along the pointer's path");
                let mut tip = overlay::Nested::new(tip);
                let node = tip.layout(&renderer, size);
                let tip_layout = Layout::new(&node);
                tip.update(
                    &event,
                    tip_layout,
                    cursor,
                    &renderer,
                    &mut clipboard::Null,
                    &mut Shell::new(&mut Vec::new()),
                );
                if tip.mouse_interaction(tip_layout, cursor, &renderer) == mouse::Interaction::None
                {
                    cursor
                } else {
                    mouse::Cursor::Unavailable
                }
            };
            widget.update(
                &mut tree,
                &event,
                layout,
                underlying_cursor,
                &renderer,
                &mut clipboard::Null,
                &mut Shell::new(&mut Vec::new()),
                &window,
            );
            assert_eq!(
                widget
                    .overlay(&mut tree, layout, &renderer, &window, Vector::ZERO)
                    .is_some(),
                stays_open
            );
        }
    }
    let position = labels.bounds(icon::glyph("close")).center();
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
            &window,
        );
    }
    assert!(matches!(
        messages.as_slice(),
        [Message::BuildHistory(build_history::Message::Close)]
    ));
}

#[tokio::test]
async fn build_history_copies_paths_generations_and_build_details() {
    #[derive(Default)]
    struct Clipboard(Option<String>);

    impl clipboard::Clipboard for Clipboard {
        fn read(&self, _kind: clipboard::Kind) -> Option<String> {
            self.0.clone()
        }

        fn write(&mut self, kind: clipboard::Kind, contents: String) {
            assert_eq!(kind, clipboard::Kind::Standard);
            self.0 = Some(contents);
        }
    }

    let mut app = boot_with_cache(Ok(None));
    let renderer = headless().await;
    let size = Size::new(800.0, 600.0);
    let store_path = "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-nixos-system";
    let revision = "0123456789abcdef0123456789abcdef01234567";
    let build: history::Build = serde_json::from_value(serde_json::json!({
        "id": "gen-build",
        "gc_root": "builds/gen-build/result",
        "store_path": store_path,
        "source": { "kind": "built", "branch": "main", "revision": revision },
        "built_at": "2026-01-02T03:04:05Z"
    }))
    .expect("build record");
    let built_at = build
        .built_at
        .unwrap()
        .with_timezone(&chrono::Local)
        .format("%-m/%-d/%Y, %-I:%M:%S %p")
        .to_string();
    let _ = app.update(Message::Overview(overview::Message::OpenBuildHistory));
    let mut snapshot = history::Snapshot::default();
    snapshot.targets.push(history::Target {
        store_path: store_path.into(),
        builds: vec![build],
        system: SystemTarget {
            generations: vec![42, 41],
            selected_generation: Some(42),
            profile_selected: true,
            active: true,
            booted: true,
        },
    });
    let _ = app.update(Message::HistoryLoaded(app.history_generation, snapshot));

    let (mut tree, _, labels) = layout(&mut app.view(), &renderer, size);
    assert_eq!(labels.focusables.len(), 6);
    let profile = app.i18n.text("history-system-profile");
    assert_eq!(
        labels
            .text
            .iter()
            .filter(|(text, _)| text == &profile)
            .count(),
        1
    );
    assert!(labels.bounds(&profile).y < labels.focusables[1].y);
    let shortcut = |key: &'static str, code| {
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Character(key.into()),
            modified_key: keyboard::Key::Character(key.into()),
            physical_key: keyboard::key::Physical::Code(code),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::CTRL,
            text: None,
            repeat: false,
        })
    };

    for (index, fragments) in [
        vec![store_path],
        vec!["generation", "42"],
        vec![store_path],
        vec!["generation", "41"],
        vec![store_path],
        vec![&built_at, "main", &revision[..8]],
    ]
    .into_iter()
    .enumerate()
    {
        let mut clipboard = Clipboard::default();
        let bounds = labels.focusables[index];
        for event in [
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            shortcut("a", keyboard::key::Code::KeyA),
            shortcut("c", keyboard::key::Code::KeyC),
        ] {
            let mut messages = Vec::new();
            {
                let mut view = app.view();
                let widget = view.as_widget_mut();
                tree.diff(&*widget);
                let node =
                    widget.layout(&mut tree, &renderer, &layout::Limits::new(Size::ZERO, size));
                widget.update(
                    &mut tree,
                    &event,
                    Layout::new(&node),
                    mouse::Cursor::Available(bounds.center()),
                    &renderer,
                    &mut clipboard,
                    &mut Shell::new(&mut messages),
                    &Rectangle::with_size(size),
                );
            }
            for message in messages {
                let _ = app.update(message);
            }
        }
        let copied = clipboard.0.expect("copied selected text");
        if fragments == [store_path] {
            assert_eq!(copied, store_path);
        }
        assert!(!copied.contains(&revision[..9]));
        for fragment in fragments {
            assert!(
                copied.contains(fragment),
                "missing {fragment:?} in {copied:?}"
            );
        }
    }
}
