use iced::{
    Font, Pixels, Rectangle, Renderer, Size,
    advanced::{
        Layout, Shell, clipboard, layout, mouse,
        renderer::Headless,
        widget::{Id, Operation, Tree},
    },
};

use super::*;

#[derive(Default)]
struct Labels(Vec<(String, Rectangle)>);

impl Operation for Labels {
    fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation)) {
        visit(self);
    }

    fn text(&mut self, _id: Option<&Id>, bounds: Rectangle, label: &str) {
        self.0.push((label.to_owned(), bounds));
    }
}

impl Labels {
    fn bounds(&self, label: &str) -> Rectangle {
        self.0
            .iter()
            .find_map(|(text, bounds)| (text == label).then_some(*bounds))
            .unwrap_or_else(|| panic!("Missing label {label:?}"))
    }

    fn contains(&self, fragment: &str) -> bool {
        self.0.iter().any(|(label, _)| label.contains(fragment))
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

    app.screen = Screen::CacheSettings(Box::new(cache_settings::State::new(None, None)));
    let labels = layout(&mut app.view(), &renderer, size).2;
    assert!(labels.contains("Could not find attic in PATH"));
    assert!(!labels.contains("Server URL"));

    app.screen = Screen::CacheSettings(Box::new(cache_settings::State::new(
        None,
        Some(PathBuf::from("/test/attic")),
    )));
    let labels = layout(&mut app.view(), &renderer, size).2;
    let field_positions = ["Server URL", "Local server name", "Token", "Cache name"]
        .map(|name| labels.bounds(name).y);
    assert!(field_positions.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(labels.contains("Attic manages the token"));

    app.set_language(LanguagePreference::Russian);
    let labels = layout(&mut app.view(), &renderer, size).2;
    assert!(labels.contains("Токеном управляет Attic"));
    assert!(labels.contains("Подключить кеш"));
}

#[tokio::test]
async fn compact_cache_settings_keep_connection_errors_and_actions_in_view() {
    use attic::tests::{NOW, fixed_time, jwt};
    use cache_settings::Message as CacheMessage;

    let mut app = boot_with_cache(Ok(None));
    app.set_language(LanguagePreference::Russian);
    let renderer = headless().await;
    let size = Size::new(560.0 / 1.5, 420.0 / 1.5);
    let toolbar = layout(&mut app.view(), &renderer, size).2;
    assert!(toolbar.bounds("Настройки кеша").x < toolbar.bounds("Настройки").x);
    assert!(toolbar.bounds("Настройки").x + toolbar.bounds("Настройки").width <= size.width);

    let mut state = cache_settings::State::new(None, Some(PathBuf::from("/test/attic")));
    let token = jwt(NOW + 3600, serde_json::json!({ "main": { "r": 1 } }));
    state.update(
        CacheMessage::UrlChanged("https://cache.example.org".into()),
        fixed_time(),
    );
    state.update(CacheMessage::TokenChanged(token), fixed_time());
    state.update(CacheMessage::CacheSelected("main".into()), fixed_time());
    assert!(matches!(
        state.update(CacheMessage::Connect, fixed_time()),
        cache_settings::Action::Connect(_, _)
    ));
    state.connected(
        &Err(attic::Failure {
            stage: attic::Stage::ApplyCache,
            kind: attic::FailureKind::ConfigWrite,
        }),
        fixed_time(),
    );
    app.screen = Screen::CacheSettings(Box::new(state));

    let labels = layout(&mut app.view(), &renderer, size).2;
    for key in [
        "cache-settings-error-config",
        "cache-settings-partial",
        "cache-settings-connect",
    ] {
        let bounds = labels.bounds(&app.i18n.text(key));
        assert!(
            bounds.y >= 0.0 && bounds.y + bounds.height <= size.height,
            "{key}: {bounds:?}"
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
