use iced::{
    Font, Pixels, Point, Rectangle, Renderer, Size, Vector,
    advanced::{
        Layout, Shell, clipboard, layout, mouse, overlay,
        renderer::Headless,
        widget::{Id, Operation, Tree, operation::Scrollable},
    },
};
use nixos_manager_apply_protocol::system::Target as SystemTarget;

use super::*;

#[derive(Default)]
struct Labels {
    text: Vec<(String, Rectangle)>,
    containers: Vec<Rectangle>,
    inputs: Vec<Rectangle>,
    focusables: Vec<Rectangle>,
    scroll_viewport: Option<(Rectangle, Vector)>,
}

impl Operation for Labels {
    fn container(&mut self, _id: Option<&Id>, bounds: Rectangle) {
        self.containers.push(bounds);
    }

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

#[test]
fn journal_collapses_only_after_successful_downloads_and_rebuilds() {
    let mut app = boot_with_cache(Ok(None));
    let config = Config {
        repository: "https://github.com/example/config".into(),
        ..Config::default()
    };
    app.config = Some(config.clone());
    app.git_available = true;
    assert!(!app.overview.journal_expanded);

    for result in [
        Ok(git::WorkingCopy::default()),
        Err(git::Error::LocalChanges),
    ] {
        let success = result.is_ok();
        let _ = app.update(Message::Overview(overview::Message::Download));
        assert!(app.overview.journal_expanded);
        let _ = app.update(Message::Downloaded(result));
        assert_eq!(app.overview.journal_expanded, !success);
    }

    for outcome in [
        Ok(()),
        Err(rebuild::Error::Exited(1)),
        Err(rebuild::Error::Interrupted),
    ] {
        app.overview.journal_expanded = false;
        let steps = rebuild::Steps::default();
        let _ = app.start_run(
            rebuild::Request::Repository {
                config: Box::new(config.clone()),
                steps,
                cached: None,
            },
            steps,
            true,
        );
        assert!(app.overview.journal_expanded);
        let _ = app.update(Message::Rebuild(rebuild::Event::Started(
            rebuild::Step::Activate,
        )));
        assert!(app.overview.journal_expanded);
        let _ = app.update(Message::Rebuild(rebuild::Event::Finished(outcome)));
        assert_eq!(app.overview.journal_expanded, outcome.is_err());
    }
}

#[tokio::test]
async fn journal_icons_keep_alignment_and_show_tooltips_above() {
    let mut app = boot_with_cache(Ok(None));
    app.set_language(LanguagePreference::Russian);
    app.rebuild = Some(rebuild::Run::new(rebuild::Steps::default(), true));
    let renderer = headless().await;

    for (size, expanded, toggle_label) in [
        (Size::new(800.0, 540.0), false, "Развернуть журнал"),
        (Size::new(560.0 / 1.5, 420.0 / 1.5), true, "Свернуть журнал"),
    ] {
        app.overview.journal_expanded = expanded;
        let mut view = app.view();
        let (mut tree, node, labels) = layout(&mut view, &renderer, size);
        let mut controls: Vec<_> = labels
            .containers
            .iter()
            .copied()
            .filter(|bounds| bounds.size() == Size::new(34.0, 34.0))
            .collect();
        controls.dedup();
        let [toggle, interrupt, copy, clear] = controls.as_slice() else {
            panic!("Expected four journal icon buttons, found {controls:?}");
        };
        let heading = labels.bounds("Логи");

        assert!((interrupt.center_x() - size.width / 2.0).abs() < 1.0);
        assert!(toggle.x + toggle.width <= heading.x);
        assert!(heading.x + heading.width <= interrupt.x);
        assert!(interrupt.x + interrupt.width <= copy.x);
        assert!(copy.x + copy.width <= clear.x);
        assert!(clear.x + clear.width < size.width);

        let layout = Layout::new(&node);
        let viewport = Rectangle::with_size(size);
        let widget = view.as_widget_mut();
        for (bounds, label) in controls.iter().zip([
            toggle_label,
            "Прервать",
            "Скопировать логи",
            "Очистить логи",
        ]) {
            assert_eq!(bounds.y, toggle.y);
            assert!(!labels.contains(label));
            let position = bounds.center();
            widget.update(
                &mut tree,
                &Event::Mouse(mouse::Event::CursorMoved { position }),
                layout,
                mouse::Cursor::Available(position),
                &renderer,
                &mut clipboard::Null,
                &mut Shell::new(&mut Vec::new()),
                &viewport,
            );
            let mut tooltip = widget
                .overlay(&mut tree, layout, &renderer, &viewport, Vector::ZERO)
                .expect("journal button tooltip on hover");
            let node = tooltip.as_overlay_mut().layout(&renderer, size);
            let mut label_layout = Layout::new(&node);
            while let Some(child) = label_layout.children().next() {
                label_layout = child;
            }
            let label_bounds = label_layout.bounds();
            assert!(label_bounds.width > 0.0);
            assert!(label_bounds.y + label_bounds.height <= bounds.y);
        }
    }
}

#[tokio::test]
async fn toolbar_settings_menu_opens_both_dialogs_without_a_repository() {
    let mut app = boot_with_cache(Ok(None));
    let renderer = headless().await;
    let size = Size::new(800.0, 540.0);
    let click = |app: &App, point: Point| {
        let mut view = app.view();
        let (mut tree, node, _) = layout(&mut view, &renderer, size);

        let mut messages = Vec::new();
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
        messages
    };

    let closed = layout(&mut app.view(), &renderer, size).2;
    let gear = closed.bounds(toolbar::SETTINGS_GEAR);
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

        let menu = layout(&mut app.view(), &renderer, size).2;
        let settings = menu.bounds(application_settings);
        assert!(gear.y + gear.height < settings.y);
        assert!(settings.y < menu.bounds(cache_settings).y);

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
    let mut changes = repository
        .iter()
        .filter(|message| !matches!(message, Settings(Repo::InputFocused)));
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
async fn compact_overview_keeps_actions_menu_to_the_left_of_download() {
    let mut app = boot_with_cache(Ok(None));
    app.config = Some(Config {
        repository: "https://github.com/example/config".into(),
        ..Config::default()
    });
    let renderer = headless().await;
    let labels = layout(&mut app.view(), &renderer, Size::new(320.0, 360.0)).2;
    let trigger = labels.bounds("☰");
    let download = labels.bounds(&app.i18n.text("overview-download"));
    assert!(trigger.x + trigger.width <= download.x);
    assert!(trigger.y <= download.center().y && download.center().y <= trigger.y + trigger.height);
}

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
        let trigger = labels.bounds("☰");
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
    let title = labels.bounds("История сборок");
    let close = labels.bounds("×");
    assert!(close.x >= title.x + title.width);
    assert!(close.y + close.height <= viewport.y);
    let refresh = labels.bounds("Обновить");
    assert!(
        refresh.y >= viewport.y + viewport.height && refresh.y + refresh.height <= compact.height
    );
    for status in ["активна", "загружена"] {
        assert!(labels.contains(status));
    }
    let profile = labels.bounds("системный профиль");
    let icon = labels.bounds("?");
    assert!(profile.x + profile.width <= icon.x);
    assert!(profile.y + profile.height <= labels.focusables[1].y);

    let (mut tree, node, labels) = layout(&mut view, &renderer, size);
    let icon = labels.bounds("?");
    let widget = view.as_widget_mut();
    let layout = Layout::new(&node);
    let window = Rectangle::with_size(size);
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
            .expect("system profile tooltip on hover");
        let mut tip = overlay::Nested::new(tip);
        let node = tip.layout(&renderer, size);
        let mut tooltip_labels = Labels::default();
        tip.operate(Layout::new(&node), &renderer, &mut tooltip_labels);
        tooltip_labels.bounds(&app.i18n.text("history-system-profile-help"))
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
            if tip.mouse_interaction(tip_layout, cursor, &renderer) == mouse::Interaction::None {
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
    let position = labels.bounds("×").center();
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
async fn build_history_activation_icon_shows_a_tooltip_and_selects_the_target() {
    let mut app = boot_with_cache(Ok(None));
    app.set_language(LanguagePreference::Russian);
    let mut state = build_history::State::default();
    let mut snapshot = history::Snapshot::default();
    let inactive_path = "/nix/store/inactive-system";
    for (path, active, generation) in [
        ("/nix/store/active-system", true, 2),
        (inactive_path, false, 1),
    ] {
        snapshot.targets.push(history::Target {
            store_path: path.into(),
            builds: Vec::new(),
            system: SystemTarget {
                generations: vec![generation],
                active,
                booted: active,
                ..SystemTarget::default()
            },
        });
    }
    state.set_snapshot(&app.i18n, &snapshot);

    let renderer = headless().await;
    let size = Size::new(560.0, 420.0);
    let viewport = Rectangle::with_size(size);
    let mut view = state
        .view(&app.i18n, Some(&snapshot), true, None, None)
        .map(Message::BuildHistory);
    let (mut tree, node, labels) = layout(&mut view, &renderer, size);
    assert_eq!(
        labels.text.iter().filter(|(text, _)| text == "▶").count(),
        1
    );
    assert!(!labels.contains("Активировать"));
    let icon = labels.bounds("▶");
    let button = labels
        .containers
        .iter()
        .find(|bounds| bounds.size() == Size::new(34.0, 34.0) && bounds.contains(icon.center()))
        .expect("activation button");
    assert!(icon.center_x() > button.center_x());
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
    assert!(matches!(
        messages.as_slice(),
        [Message::BuildHistory(build_history::Message::Select(rebuild::ReadyTarget::Selected(path)))]
            if path == std::path::Path::new(inactive_path)
    ));
}

#[tokio::test]
async fn build_history_toggles_removal_for_an_old_duplicate_generation() {
    #[derive(Clone, Copy)]
    enum Control {
        Trash,
        Remove,
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
        let refresh = labels.bounds("Обновить");
        assert!(remove.x < refresh.x);
        assert!(remove.y + remove.height <= size.height);
        assert!(refresh.y + refresh.height <= size.height);
        assert_eq!(
            labels.contains(&app.i18n.text("history-removal-boot-warning")),
            marked
        );
        let close = labels.bounds("×").center();
        let mut controls: Vec<_> = labels
            .containers
            .into_iter()
            .filter(|bounds| bounds.size() == Size::new(34.0, 34.0) && !bounds.contains(close))
            .collect();
        controls.dedup();
        let [trash] = controls.as_slice() else {
            panic!("expected one trash button, found {controls:?}")
        };
        let position = match control {
            Control::Trash => trash.center(),
            Control::Remove => remove.center(),
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
                (Control::Trash, mouse::Event::CursorMoved { .. })
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
    assert!(interact(&state, size, Control::Remove, false).is_empty());
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
    state.removal_finished(Err(rebuild::Error::Cancelled));
    state.set_snapshot(&app.i18n, &snapshot);
    let compact = Size::new(560.0 / 1.5, 170.0);
    assert!(matches!(
        interact(&state, compact, Control::Remove, true).as_slice(),
        [Message::BuildHistory(build_history::Message::Remove)]
    ));
    snapshot.targets[0].system.generations = vec![42];
    state.set_snapshot(&app.i18n, &snapshot);
    assert!(matches!(
        state.update(build_history::Message::Remove, Some(&snapshot), false, true),
        build_history::Action::None
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
