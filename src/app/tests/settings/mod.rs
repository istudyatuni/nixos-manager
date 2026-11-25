use iced::{
    Event, Point, Rectangle, Size, Vector,
    advanced::{Layout, Shell, clipboard, mouse, overlay},
    event, keyboard, window,
};

use crate::{
    app::{App, Message},
    config::Config,
    i18n::LanguagePreference,
    ui::{
        icon,
        screens::{Screen, cache_settings, settings},
    },
};

use super::support::{Labels, boot_with_cache, headless, layout};

mod branches;

#[tokio::test]
async fn settings_show_an_invalid_repository_url_under_the_field_on_blur() {
    use iced::advanced::widget::{Operation, operation};

    let renderer = headless().await;
    let size = Size::new(800.0, 1000.0);
    let cases = [
        (
            "http://github.com/example/repository",
            "Enter an HTTPS link to a GitHub repository",
            false,
        ),
        ("just some text", "Enter a valid URL", false),
        (
            "https://gitlab.com/example/repository",
            "Enter a link with the github.com domain",
            false,
        ),
        (
            "https://git hub.com/example/repository",
            "Enter a link with the github.com domain",
            false,
        ),
        (
            "https://github.com/example",
            "Enter a link in the format https://github.com/owner/repo",
            true,
        ),
    ];
    for (url, hint, use_tab) in cases {
        let mut app = boot_with_cache(Ok(None));
        let _ = app.update(Message::OpenSettings);
        let _ = app.update(Message::Settings(settings::Message::RepositoryChanged(
            url.into(),
        )));

        let messages = {
            let mut view = app.view();
            let (mut tree, node, labels) = layout(&mut view, &renderer, size);
            assert!(!labels.contains(hint));
            let widget = view.as_widget_mut();
            let root = Layout::new(&node);
            let mut messages = Vec::new();
            let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
            widget.update(
                &mut tree,
                &press,
                root,
                mouse::Cursor::Available(labels.inputs[0].center()),
                &renderer,
                &mut clipboard::Null,
                &mut Shell::new(&mut messages),
                &Rectangle::with_size(size),
            );
            assert!(messages.is_empty());

            let (event, cursor) = if use_tab {
                let mut focus_next = operation::focusable::focus_next::<()>();
                widget.operate(&mut tree, root, &renderer, &mut focus_next);
                let operation::Outcome::Chain(mut move_focus) = focus_next.finish() else {
                    panic!("expected a focus traversal");
                };
                widget.operate(&mut tree, root, &renderer, move_focus.as_mut());
                (
                    Event::Window(window::Event::RedrawRequested(iced::time::Instant::now())),
                    mouse::Cursor::Unavailable,
                )
            } else {
                (press, mouse::Cursor::Available(labels.inputs[1].center()))
            };
            widget.update(
                &mut tree,
                &event,
                root,
                cursor,
                &renderer,
                &mut clipboard::Null,
                &mut Shell::new(&mut messages),
                &Rectangle::with_size(size),
            );
            messages
        };
        assert!(messages.iter().any(|message| matches!(
            message,
            Message::Settings(settings::Message::RepositoryBlurred)
        )));
        for message in messages {
            let _ = app.update(message);
        }
        let labels = layout(&mut app.view(), &renderer, size).2;
        let hint_bounds = labels.bounds(hint);
        let input = labels.inputs[0];
        assert!(input.y + input.height <= hint_bounds.y);
        assert!(hint_bounds.y < labels.bounds("GitHub access token").y);

        let _ = app.update(Message::Settings(settings::Message::RepositoryChanged(
            "https://github.com/example/repository".into(),
        )));
        let _ = app.update(Message::Settings(settings::Message::RepositoryBlurred));
        let labels = layout(&mut app.view(), &renderer, size).2;
        assert!(!labels.contains(hint));
    }
}

#[tokio::test]
async fn toolbar_settings_menu_opens_both_dialogs_without_a_repository() {
    let mut app = boot_with_cache(Ok(None));
    let renderer = headless().await;
    let size = Size::new(800.0, 540.0);
    let viewport = Rectangle::with_size(size);
    let click = |app: &App, point: Point| {
        let mut view = app.view();
        let (mut tree, node, _) = layout(&mut view, &renderer, size);
        let root = Layout::new(&node);
        let cursor = mouse::Cursor::Available(point);
        let events = [
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ];
        let mut messages = Vec::new();
        let widget = view.as_widget_mut();
        if let Some(overlay) = widget.overlay(&mut tree, root, &renderer, &viewport, Vector::ZERO) {
            let mut overlay = overlay::Nested::new(overlay);
            let node = overlay.layout(&renderer, size);
            for event in &events {
                overlay.update(
                    event,
                    Layout::new(&node),
                    cursor,
                    &renderer,
                    &mut clipboard::Null,
                    &mut Shell::new(&mut messages),
                );
            }
            return messages;
        }
        for event in &events {
            widget.update(
                &mut tree,
                event,
                root,
                cursor,
                &renderer,
                &mut clipboard::Null,
                &mut Shell::new(&mut messages),
                &viewport,
            );
        }
        messages
    };

    let closed = layout(&mut app.view(), &renderer, size).2;
    let gear = closed.bounds(icon::glyph("settings"));
    for point in [gear.center(), closed.bounds(&app.title()).center()] {
        let _ = app.update(Message::OpenSettingsMenu);
        let messages = click(&app, point);
        assert!(matches!(messages.as_slice(), [Message::DismissDialog]));
        let _ = app.update(messages.into_iter().next().unwrap());
        assert!(!app.settings_menu_open);
    }

    let application_settings = "Application Settings";
    let cache_settings = "Cache Settings";
    for (label, expected) in [
        (application_settings, Message::OpenSettings),
        (cache_settings, Message::OpenCacheSettings),
    ] {
        let closed = layout(&mut app.view(), &renderer, size).2;
        assert!(
            closed
                .text
                .iter()
                .all(|(label, _)| label != application_settings && label != cache_settings)
        );
        let messages = click(&app, gear.center());
        assert!(matches!(messages.as_slice(), [Message::OpenSettingsMenu]));
        let _ = app.update(messages.into_iter().next().unwrap());

        let menu = {
            let mut view = app.view();
            let (mut tree, node, background) = layout(&mut view, &renderer, size);
            assert_eq!(background.bounds(&app.title()), closed.bounds(&app.title()));
            let overlay = view
                .as_widget_mut()
                .overlay(
                    &mut tree,
                    Layout::new(&node),
                    &renderer,
                    &viewport,
                    Vector::ZERO,
                )
                .expect("settings menu");
            let mut overlay = overlay::Nested::new(overlay);
            let node = overlay.layout(&renderer, size);
            let mut labels = Labels::default();
            overlay.operate(Layout::new(&node), &renderer, &mut labels);
            labels
        };
        let settings = menu.bounds(application_settings);
        assert!(gear.y + gear.height < settings.y);
        assert!(settings.y < menu.bounds(cache_settings).y);
        let panel = menu.containers[0];
        assert_eq!(panel.x + panel.width, gear.x + gear.width);

        let messages = click(&app, menu.bounds(label).center());
        assert_eq!(messages.len(), 1);
        assert_eq!(
            std::mem::discriminant(&messages[0]),
            std::mem::discriminant(&expected)
        );
        let _ = app.update(messages.into_iter().next().unwrap());
        assert!(!app.settings_menu_open);
        if matches!(expected, Message::OpenSettings) {
            assert!(matches!(app.screen, Screen::Settings(_)));
            let dialog = layout(&mut app.view(), &renderer, size).2;
            assert!(dialog.text.iter().any(|(title, _)| title == label));
            let _ = app.update(Message::Settings(settings::Message::Cancel));
        }
    }
    assert!(app.config.is_none());
    assert!(matches!(app.screen, Screen::CacheSettings(_)));

    app.screen = Screen::CacheSettings(Box::new(cache_settings::State::new(None, false)));
    let labels = layout(&mut app.view(), &renderer, size).2;
    assert!(labels.contains("Could not find attic in PATH"));
    assert!(!labels.contains("Server URL"));

    app.screen = Screen::CacheSettings(Box::new(cache_settings::State::new(None, true)));
    let labels = layout(&mut app.view(), &renderer, size).2;
    let field_positions = ["Server URL", "Local server name", "Token", "Cache name"]
        .map(|name| labels.bounds(name).y);
    assert!(field_positions.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(labels.contains("Attic manages the token. Use this field to enter a new token."));

    app.set_language(LanguagePreference::Russian);
    let labels = layout(&mut app.view(), &renderer, size).2;
    assert!(
        labels.contains("Токеном управляет Attic. Поле предназначено для ввода нового токена.")
    );
    assert!(labels.contains("Подключить кеш"));
}

#[tokio::test]
async fn escape_cancels_application_settings_after_dismissing_the_branch_picker() {
    use iced::{advanced::subscription, futures::StreamExt};

    let mut app = boot_with_cache(Ok(None));
    app.config = Some(Config {
        repository: "https://github.com/example/repository".into(),
        ..Config::default()
    });
    app.git_available = true;
    let renderer = headless().await;
    let size = Size::new(800.0, 1000.0);
    let key = keyboard::Key::Named(keyboard::key::Named::Escape);
    let escape = Event::Keyboard(keyboard::Event::KeyPressed {
        key: key.clone(),
        modified_key: key,
        physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Escape),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::default(),
        text: None,
        repeat: false,
    });

    for (label, open_picker) in [("GitHub repository URL", false), ("Branch", true)] {
        let _ = app.update(Message::OpenSettings);
        let _ = app.update(Message::Settings(settings::Message::ScaleChanged(125)));
        assert_eq!(app.scale_factor(), 1.25);
        let status = {
            let mut view = app.view();
            let (mut tree, node, labels) = layout(&mut view, &renderer, size);
            let label = labels.bounds(label);
            let input = labels
                .inputs
                .iter()
                .find(|input| input.y > label.y)
                .unwrap();
            let point = if open_picker {
                Point::new(input.x + input.width - 12.0, input.center_y())
            } else {
                input.center()
            };
            let mut dispatch = |event: &Event| {
                let mut messages = Vec::new();
                let mut shell = Shell::new(&mut messages);
                view.as_widget_mut().update(
                    &mut tree,
                    event,
                    Layout::new(&node),
                    mouse::Cursor::Available(point),
                    &renderer,
                    &mut clipboard::Null,
                    &mut shell,
                    &Rectangle::with_size(size),
                );
                shell.event_status()
            };
            assert_eq!(
                dispatch(&Event::Mouse(mouse::Event::ButtonPressed(
                    mouse::Button::Left
                ))),
                event::Status::Captured
            );
            if open_picker {
                assert_eq!(dispatch(&escape), event::Status::Captured);
            }
            dispatch(&escape)
        };
        let interaction = subscription::Event::Interaction {
            window: window::Id::unique(),
            event: escape.clone(),
            status,
        };
        let messages: Vec<_> = iced::futures::stream::select_all(
            subscription::into_recipes(app.subscription())
                .into_iter()
                .map(|recipe| {
                    recipe.stream(Box::pin(iced::futures::stream::iter([interaction.clone()])))
                }),
        )
        .collect()
        .await;
        assert!(matches!(messages.as_slice(), [Message::DismissDialog]));
        let _ = app.update(messages.into_iter().next().unwrap());
        assert!(matches!(app.screen, Screen::Overview));
        assert_eq!(app.scale_factor(), 1.0);
    }
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn selected_settings_text_can_be_pasted_into_another_field() {
    struct PrimaryClipboard(String);

    impl clipboard::Clipboard for PrimaryClipboard {
        fn read(&self, kind: clipboard::Kind) -> Option<String> {
            assert_eq!(kind, clipboard::Kind::Primary);
            Some(self.0.clone())
        }

        fn write(&mut self, kind: clipboard::Kind, contents: String) {
            assert_eq!(kind, clipboard::Kind::Primary);
            self.0 = contents;
        }
    }

    let mut app = boot_with_cache(Ok(None));
    let path = "/tmp/repository";
    app.config = Some(Config {
        repository: "https://github.com/example/repository".into(),
        path: path.into(),
        ..Config::default()
    });
    let _ = app.update(Message::OpenSettings);
    let renderer = headless().await;
    let size = Size::new(800.0, 1000.0);
    let mut view = app.view();
    let (mut tree, node, labels) = layout(&mut view, &renderer, size);
    let field = |label| {
        let label = labels.bounds(label);
        *labels
            .inputs
            .iter()
            .find(|input| input.y > label.y)
            .unwrap()
    };
    let source = field("Local path");
    let target = field("Subdirectory containing flake.nix");
    let start = Point::new(source.x + 2.0, source.center_y());
    let end = source.center();
    let mut dispatch = |event, point, clipboard: &mut PrimaryClipboard| {
        let mut messages = Vec::new();
        view.as_widget_mut().update(
            &mut tree,
            &event,
            Layout::new(&node),
            mouse::Cursor::Available(point),
            &renderer,
            clipboard,
            &mut Shell::new(&mut messages),
            &Rectangle::with_size(size),
        );
        messages
    };
    let mut clipboard = PrimaryClipboard(String::new());
    for (event, position) in [
        (mouse::Event::ButtonPressed(mouse::Button::Left), start),
        (mouse::Event::CursorMoved { position: end }, end),
        (mouse::Event::ButtonReleased(mouse::Button::Left), end),
    ] {
        dispatch(Event::Mouse(event), position, &mut clipboard);
    }
    assert_eq!(clipboard.0, path);

    // Another application can claim PRIMARY while the local selection stays visible.
    clipboard.0 = "external selection".into();
    let ctrl = keyboard::Modifiers::CTRL;
    dispatch(
        Event::Keyboard(keyboard::Event::ModifiersChanged(ctrl)),
        end,
        &mut clipboard,
    );
    let key = keyboard::Key::Character("a".into());
    dispatch(
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::KeyA),
            location: keyboard::Location::Standard,
            modifiers: ctrl,
            text: None,
            repeat: false,
        }),
        end,
        &mut clipboard,
    );
    assert_eq!(clipboard.0, path);

    let mut changes = dispatch(
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Middle)),
        target.center(),
        &mut clipboard,
    )
    .into_iter()
    .filter(|message| !matches!(message, Message::Settings(settings::Message::InputFocused)));
    assert!(matches!(
        changes.next(),
        Some(Message::Settings(settings::Message::FlakeDirectoryChanged(value))) if value == path
    ));
    assert!(changes.next().is_none());
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn middle_click_pastes_primary_into_editable_settings_fields() {
    #[derive(Default)]
    struct PrimaryClipboard(std::cell::Cell<usize>);

    impl clipboard::Clipboard for PrimaryClipboard {
        fn read(&self, kind: clipboard::Kind) -> Option<String> {
            assert_eq!(kind, clipboard::Kind::Primary);
            self.0.set(self.0.get() + 1);
            Some("é\nX\t".to_owned())
        }

        fn write(&mut self, _kind: clipboard::Kind, _contents: String) {
            panic!("pasting should not write to the clipboard");
        }
    }

    let mut app = boot_with_cache(Ok(None));
    app.config = Some(Config {
        repository: "https://github.com/example/repository".into(),
        branch: "main".into(),
        path: "/tmp/repository".into(),
        ..Config::default()
    });
    app.git_available = true;
    let _ = app.update(Message::OpenSettings);
    let renderer = headless().await;
    let size = Size::new(800.0, 1000.0);
    let mut clipboard = PrimaryClipboard::default();
    let mut paste = |app: &App, fields: &[&str]| {
        let mut view = app.view();
        let (mut tree, node, labels) = layout(&mut view, &renderer, size);
        let mut messages = Vec::new();
        for field in fields {
            let label = labels.bounds(field);
            let input = labels
                .inputs
                .iter()
                .find(|input| input.y > label.y)
                .unwrap();
            view.as_widget_mut().update(
                &mut tree,
                &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Middle)),
                Layout::new(&node),
                mouse::Cursor::Available(Point::new(input.x + 2.0, input.center_y())),
                &renderer,
                &mut clipboard,
                &mut Shell::new(&mut messages),
                &Rectangle::with_size(size),
            );
        }
        messages
    };

    use Message::{CacheSettings, Settings};
    use cache_settings::Message as Cache;
    use settings::Message as Repo;
    let repository = paste(
        &app,
        &[
            "GitHub repository URL",
            "GitHub access token",
            "Local path",
            "Branch",
            "Subdirectory containing flake.nix",
        ],
    );
    let mut changes = repository.iter().filter(|message| {
        !matches!(
            message,
            Settings(Repo::InputFocused | Repo::RepositoryBlurred)
        )
    });
    assert!(matches!(
        changes.next(),
        Some(Settings(Repo::RepositoryChanged(value))) if value == "éXhttps://github.com/example/repository"
    ));
    assert!(matches!(changes.next(), Some(Settings(Repo::TokenChanged(value))) if value == "éX"));
    assert!(matches!(
        changes.next(),
        Some(Settings(Repo::PathChanged(value))) if value == "éX/tmp/repository"
    ));
    assert!(
        matches!(changes.next(), Some(Settings(Repo::BranchChanged(value))) if value == "éXmain")
    );
    assert!(matches!(
        changes.next(),
        Some(Settings(Repo::FlakeDirectoryChanged(value))) if value == "éX"
    ));
    assert!(changes.next().is_none());

    app.screen = Screen::CacheSettings(Box::new(cache_settings::State::new(None, true)));
    let cache = paste(
        &app,
        &["Server URL", "Local server name", "Token", "Cache name"],
    );
    assert!(matches!(
        cache.as_slice(),
        [CacheSettings(Cache::UrlChanged(url)), CacheSettings(Cache::ServerNameChanged(name)), CacheSettings(Cache::TokenChanged(token))]
            if url == "éX" && name == "éXprivate" && token == "éX"
    ));
    assert_eq!(clipboard.0.get(), 8);
}
