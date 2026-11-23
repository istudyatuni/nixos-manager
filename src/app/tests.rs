use iced::{
    Font, Pixels, Point, Rectangle, Renderer, Size, Vector,
    advanced::{
        Layout, Shell, clipboard, layout, mouse,
        renderer::Headless,
        widget::{Id, Operation, Tree, operation::Scrollable},
    },
};

use super::*;

#[derive(Default)]
struct Labels {
    text: Vec<(String, Rectangle)>,
    inputs: Vec<Rectangle>,
    focusables: Vec<Rectangle>,
    scroll_viewport: Option<(Rectangle, Vector)>,
}

impl Operation for Labels {
    fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation)) {
        visit(self);
    }

    fn text(&mut self, _id: Option<&Id>, bounds: Rectangle, label: &str) {
        self.text.push((label.to_owned(), bounds));
    }

    fn text_input(
        &mut self,
        _id: Option<&Id>,
        bounds: Rectangle,
        _state: &mut dyn iced::advanced::widget::operation::TextInput,
    ) {
        self.inputs.push(bounds);
    }

    fn focusable(
        &mut self,
        _id: Option<&Id>,
        bounds: Rectangle,
        _state: &mut dyn iced::advanced::widget::operation::Focusable,
    ) {
        self.focusables.push(bounds);
    }

    fn scrollable(
        &mut self,
        _id: Option<&Id>,
        bounds: Rectangle,
        _content_bounds: Rectangle,
        translation: Vector,
        _state: &mut dyn Scrollable,
    ) {
        self.scroll_viewport = Some((bounds, translation));
    }
}

impl Labels {
    fn bounds(&self, label: &str) -> Rectangle {
        self.text
            .iter()
            .find_map(|(text, bounds)| (text == label).then_some(*bounds))
            .unwrap_or_else(|| panic!("Missing label {label:?}"))
    }

    fn contains(&self, fragment: &str) -> bool {
        self.text.iter().any(|(label, _)| label.contains(fragment))
    }
}

fn layout(
    view: &mut Element<'_, Message>,
    renderer: &Renderer,
    size: Size,
) -> (Tree, layout::Node, Labels) {
    let widget = view.as_widget_mut();
    let mut tree = Tree::new(&*widget);
    let node = widget.layout(&mut tree, renderer, &layout::Limits::new(Size::ZERO, size));
    let mut labels = Labels::default();
    widget.operate(&mut tree, Layout::new(&node), renderer, &mut labels);
    (tree, node, labels)
}

async fn headless() -> Renderer {
    <Renderer as Headless>::new(Font::DEFAULT, Pixels(16.0), Some("tiny-skia"))
        .await
        .expect("software renderer")
}

fn boot_with_cache(cache: Result<Option<attic::Metadata>, attic::LoadError>) -> App {
    let (mut app, _) = App::boot();
    let _ = app.update(Message::Loaded(
        Ok(None),
        cache,
        false,
        rebuild::Tools::default(),
    ));
    app.set_language(LanguagePreference::English);
    app
}

#[tokio::test]
async fn cache_toolbar_button_opens_a_separate_dialog_without_a_repository() {
    let mut app = boot_with_cache(Ok(None));
    let renderer = headless().await;
    let size = Size::new(800.0, 540.0);
    let messages = {
        let mut view = app.view();
        let (mut tree, node, labels) = layout(&mut view, &renderer, size);
        let button = labels.bounds("Cache Settings");
        assert!(button.x + button.width < labels.bounds("Settings").x);

        let mut messages = Vec::new();
        for event in [
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ] {
            view.as_widget_mut().update(
                &mut tree,
                &Event::Mouse(event),
                Layout::new(&node),
                mouse::Cursor::Available(button.center()),
                &renderer,
                &mut clipboard::Null,
                &mut Shell::new(&mut messages),
                &Rectangle::with_size(size),
            );
        }
        messages
    };
    assert!(matches!(messages.as_slice(), [Message::OpenCacheSettings]));
    let _ = app.update(messages.into_iter().next().unwrap());
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
async fn settings_request_remote_branches_when_the_branch_picker_opens() {
    let mut app = boot_with_cache(Ok(None));
    app.config = Some(Config {
        repository: "https://github.com/example/repository".to_owned(),
        branch: "main".to_owned(),
        ..Config::default()
    });
    app.git_available = true;
    assert_eq!(app.update(Message::OpenSettings).units(), 1);
    let _ = app.update(Message::SettingsLocalBranches(
        app.settings_lookup_generation,
        Vec::new(),
    ));

    let renderer = headless().await;
    let size = Size::new(800.0, 1000.0);
    let messages = {
        let mut view = app.view();
        let (mut tree, node, labels) = layout(&mut view, &renderer, size);
        let branch_label = labels.bounds("Branch");
        let branch_input = labels
            .inputs
            .iter()
            .find(|bounds| bounds.y > branch_label.y)
            .unwrap();
        let mut messages = Vec::new();
        for point in [
            branch_input.center(),
            Point::new(
                branch_input.x + branch_input.width - 12.0,
                branch_input.center_y(),
            ),
        ] {
            view.as_widget_mut().update(
                &mut tree,
                &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                Layout::new(&node),
                mouse::Cursor::Available(point),
                &renderer,
                &mut clipboard::Null,
                &mut Shell::new(&mut messages),
                &Rectangle::with_size(size),
            );
        }
        messages
    };
    assert!(matches!(
        messages.as_slice(),
        [
            Message::Settings(settings::Message::InputFocused),
            Message::Settings(settings::Message::BranchPickerOpened)
        ]
    ));
    for (message, expected_tasks) in messages.into_iter().zip([0, 1]) {
        assert_eq!(app.update(message).units(), expected_tasks);
    }
    assert_eq!(
        app.update(Message::Settings(settings::Message::BranchPickerOpened))
            .units(),
        0
    );
    let _ = app.update(Message::SettingsRemoteRefs(
        app.settings_lookup_generation,
        Ok(git::RemoteRefs {
            branches: Some(Vec::new()),
            default_branch: Some("main".into()),
        }),
    ));
    assert_eq!(
        app.update(Message::Settings(settings::Message::BranchPickerOpened))
            .units(),
        0
    );
}

#[tokio::test]
async fn cache_settings_show_the_expiration_and_actions_in_order() {
    let metadata = attic::Metadata {
        cache_name: "main".into(),
        cache_expiration: "2026-07-07T00:30:45Z".parse().unwrap(),
        server_url: "https://cache.example.org/".into(),
        server_name: "private".into(),
    };
    let mut app = boot_with_cache(Ok(Some(metadata.clone())));
    app.screen = Screen::CacheSettings(Box::new(cache_settings::State::new(Some(&metadata), true)));
    let renderer = headless().await;
    let size = Size::new(800.0, 1000.0);
    let expiration_local = metadata.cache_expiration.with_timezone(&chrono::Local);
    for (language, pattern) in [
        (LanguagePreference::English, "%-m/%-d/%Y, %-I:%M:%S %p"),
        (LanguagePreference::Russian, "%d.%m.%Y, %H:%M:%S"),
    ] {
        app.set_language(language);
        let labels = layout(&mut app.view(), &renderer, size).2;
        let expected = expiration_local.format(pattern).to_string();
        let expiration = labels
            .text
            .iter()
            .find(|(label, _)| label.contains(&expected))
            .unwrap()
            .1;
        let update = labels.bounds(&app.i18n.text("cache-settings-update"));
        let changes = labels.bounds(&app.i18n.text("cache-settings-config-changes"));
        let forget = app.i18n.text("cache-settings-forget");
        let forget_button = labels
            .text
            .iter()
            .rfind(|(label, _)| label == &forget)
            .unwrap()
            .1;
        let positions = [
            expiration,
            labels.bounds(&app.i18n.text("cache-settings-config-details")),
            update,
            labels.bounds(&forget),
            labels.bounds(&app.i18n.text("cache-settings-forget-hint")),
            labels.bounds(&app.i18n.text("cache-settings-files")),
            forget_button,
        ];
        assert!(positions.windows(2).all(|pair| pair[0].y < pair[1].y));
        assert!(changes.x + changes.width <= update.x);
        assert!((changes.center_y() - update.center_y()).abs() < 1.0);
        assert!(update.center().x > size.width / 2.0);
        assert!((forget_button.x + forget_button.width - (update.x + update.width)).abs() < 1.0);
        assert!(labels.contains("×"));
    }
}

#[tokio::test]
async fn cache_settings_offer_the_guide_when_no_cache_is_saved() {
    let mut app = boot_with_cache(Ok(None));
    app.screen = Screen::CacheSettings(Box::new(cache_settings::State::new(None, true)));
    let renderer = headless().await;
    let size = Size::new(800.0, 1000.0);
    let messages = {
        let mut view = app.view();
        let (mut tree, node, labels) = layout(&mut view, &renderer, size);
        let files = labels.bounds(&app.i18n.text("cache-settings-files"));
        let forget = app.i18n.text("cache-settings-forget");
        let forget_button = labels
            .text
            .iter()
            .rfind(|(label, _)| label == &forget)
            .unwrap()
            .1;
        let link = Point::new(files.x + 12.0, forget_button.center_y());
        assert!(link.x < forget_button.x && link.y > files.y + files.height);
        let mut messages = Vec::new();
        for point in [link, forget_button.center()] {
            for event in [
                mouse::Event::ButtonPressed(mouse::Button::Left),
                mouse::Event::ButtonReleased(mouse::Button::Left),
            ] {
                view.as_widget_mut().update(
                    &mut tree,
                    &Event::Mouse(event),
                    Layout::new(&node),
                    mouse::Cursor::Available(point),
                    &renderer,
                    &mut clipboard::Null,
                    &mut Shell::new(&mut messages),
                    &Rectangle::with_size(size),
                );
            }
        }
        messages
    };
    assert!(matches!(
        messages.as_slice(),
        [Message::CacheSettings(cache_settings::Message::OpenGuide)]
    ));
}

#[tokio::test]
async fn compact_cache_settings_can_scroll_to_connection_errors() {
    use attic::tests::{NOW, fixed_time, jwt};
    use cache_settings::Message as CacheMessage;

    let mut app = boot_with_cache(Ok(None));
    app.set_language(LanguagePreference::Russian);
    let renderer = headless().await;
    let size = Size::new(560.0 / 1.5, 420.0 / 1.5);
    let toolbar = layout(&mut app.view(), &renderer, size).2;
    assert!(toolbar.bounds("Настройки кеша").x < toolbar.bounds("Настройки").x);
    assert!(toolbar.bounds("Настройки").x + toolbar.bounds("Настройки").width <= size.width);

    let mut state = cache_settings::State::new(None, true);
    let token = jwt(NOW + 3600, serde_json::json!({ "main": { "r": 1 } }));
    state.update(
        CacheMessage::UrlChanged("https://cache.example.org".into()),
        fixed_time(),
    );
    state.update(CacheMessage::TokenChanged(token), fixed_time());
    state.update(CacheMessage::CacheSelected("main".into()), fixed_time());
    assert!(matches!(
        state.update(CacheMessage::Connect, fixed_time()),
        cache_settings::Action::Connect(_)
    ));
    state.connected(
        &Err(attic::Failure {
            stage: attic::Stage::ApplyCache,
            kind: attic::FailureKind::ConfigWrite,
        }),
        fixed_time(),
    );
    app.screen = Screen::CacheSettings(Box::new(state));

    let mut view = app.view();
    let (mut tree, node, labels) = layout(&mut view, &renderer, size);
    let (viewport, _) = labels.scroll_viewport.unwrap();
    let button = labels.bounds(&app.i18n.text("cache-settings-connect"));
    let error = labels.bounds(&app.i18n.text("cache-settings-error-config"));
    assert!(button.y + button.height <= error.y);
    let scroll = error.y - viewport.y - 4.0;
    let mut messages = Vec::new();
    view.as_widget_mut().update(
        &mut tree,
        &Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Pixels { x: 0.0, y: -scroll },
        }),
        Layout::new(&node),
        mouse::Cursor::Available(viewport.center()),
        &renderer,
        &mut clipboard::Null,
        &mut Shell::new(&mut messages),
        &Rectangle::with_size(size),
    );
    let mut labels = Labels::default();
    view.as_widget_mut()
        .operate(&mut tree, Layout::new(&node), &renderer, &mut labels);
    let (_, translation) = labels.scroll_viewport.unwrap();
    assert!(translation.y > 0.0);
    for key in ["cache-settings-error-config", "cache-settings-partial"] {
        let bounds = labels.bounds(&app.i18n.text(key));
        assert!(
            bounds.y - translation.y >= viewport.y
                && bounds.y + bounds.height - translation.y <= viewport.y + viewport.height,
            "{key}: {bounds:?}, viewport {viewport:?}, scroll {translation:?}"
        );
    }
}

#[tokio::test]
async fn overview_shows_saved_cache_warnings_without_a_repository_or_cli() {
    let renderer = headless().await;
    let size = Size::new(800.0, 540.0);
    let expired = chrono::DateTime::from_timestamp(1_705_000_000, 0).unwrap();
    let metadata = attic::Metadata {
        cache_name: "main".to_owned(),
        cache_expiration: expired,
        server_url: "https://cache.example.org/".to_owned(),
        server_name: "private".to_owned(),
    };

    let mut app = boot_with_cache(Ok(Some(metadata.clone())));
    let labels = layout(&mut app.view(), &renderer, size).2;
    assert!(labels.contains("expired at"));
    assert!(labels.contains(&metadata.expiration_rfc3339()));
    assert!(labels.contains("main"));
    assert!(app.config.is_none());

    let mut renewed = metadata;
    renewed.cache_expiration += chrono::TimeDelta::hours(1);
    app.set_cache(Ok(Some(renewed.clone())), expired);
    assert!(
        layout(&mut app.view(), &renderer, size)
            .2
            .contains("expires soon")
    );

    renewed.cache_expiration += chrono::TimeDelta::hours(23);
    app.set_cache(Ok(Some(renewed)), expired);
    assert!(
        !layout(&mut app.view(), &renderer, size)
            .2
            .contains("expires soon")
    );
    app.set_cache(Err(attic::LoadError), expired);
    assert!(
        layout(&mut app.view(), &renderer, size)
            .2
            .contains("Could not read the saved cache details")
    );
    let _ = app.update(Message::OpenCacheSettings);
    let _ = app.update(Message::CacheSettings(cache_settings::Message::Forget));
    let _ = app.update(Message::CacheForgotten(true));
    assert!(app.cache.is_ok_and(|cache| cache.is_none()));
    assert!(app.cache_expiration_warning.is_none());
}

#[tokio::test]
async fn build_history_shows_system_configurations_without_a_repository() {
    let mut app = boot_with_cache(Ok(None));
    let renderer = headless().await;
    let size = Size::new(800.0, 540.0);
    let labels = layout(&mut app.view(), &renderer, size).2;
    assert!(labels.contains("Build History"));

    let _ = app.update(Message::Overview(overview::Message::OpenBuildHistory));
    assert!(matches!(app.screen, Screen::BuildHistory(_)));
    let mut snapshot = history::Snapshot::default();
    snapshot
        .targets
        .extend((42..57).map(|number| history::Target {
            store_path: format!("/nix/store/example-{number}").into(),
            builds: Vec::new(),
            generations: vec![number],
            profile_selected: number == 42,
            selected_generation: (number == 42).then_some(number),
            active: number == 42,
            booted: number == 42,
        }));
    let _ = app.update(Message::HistoryLoaded(app.history_generation, snapshot));

    app.set_language(LanguagePreference::Russian);
    let compact = Size::new(560.0 / 1.5, 420.0 / 1.5);
    let labels = layout(&mut app.view(), &renderer, compact).2;
    let (viewport, _) = labels.scroll_viewport.expect("scrollable build history");
    let last_generation = labels.focusables.last().expect("selectable generation");
    assert!(last_generation.y > viewport.y + viewport.height);
    let close = labels.bounds("Закрыть");
    assert!(close.y >= viewport.y + viewport.height && close.y + close.height <= compact.height);
    for status in ["активна", "загружена"] {
        assert!(labels.contains(status));
    }
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
    let size = Size::new(800.0, 540.0);
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
        generations: vec![42, 41],
        selected_generation: Some(42),
        profile_selected: true,
        active: true,
        booted: true,
    });
    let _ = app.update(Message::HistoryLoaded(app.history_generation, snapshot));

    let (mut tree, _, labels) = layout(&mut app.view(), &renderer, size);
    assert_eq!(labels.focusables.len(), 4);
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
        vec!["42", "selected in system profile"],
        vec!["41"],
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
        if index == 0 {
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
