use iced::{
    Event, Point, Rectangle, Size,
    advanced::{Layout, Shell, clipboard, mouse},
};

use crate::{
    app::{App, Message},
    attic,
    i18n::LanguagePreference,
    ui::{
        icon,
        screens::{Screen, cache_settings},
    },
};

use super::support::{Labels, boot_with_cache, headless, layout};

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
        assert!(labels.contains(icon::glyph("close")));
    }
}

#[tokio::test]
async fn cache_settings_require_confirmation_before_forgetting() {
    let metadata = attic::Metadata {
        cache_name: "main".into(),
        cache_expiration: attic::now(),
        server_url: "https://cache.example.org/".into(),
        server_name: "private".into(),
    };
    let mut app = boot_with_cache(Ok(Some(metadata.clone())));
    app.screen =
        Screen::CacheSettings(Box::new(cache_settings::State::new(Some(&metadata), false)));
    app.set_language(LanguagePreference::Russian);

    let renderer = headless().await;
    let settings_size = Size::new(800.0, 540.0);
    let confirm_size = Size::new(560.0 / 1.5, 420.0 / 1.5);
    let click = |app: &mut App, label, size| {
        let viewport = Rectangle::with_size(size);
        let mut messages = {
            let mut view = app.view();
            let (mut tree, node, labels) = layout(&mut view, &renderer, size);
            let bounds = labels
                .text
                .iter()
                .rfind(|(text, _)| text == label)
                .unwrap()
                .1;
            assert!(viewport.contains(bounds.center()));
            let mut messages = Vec::new();
            for event in [
                mouse::Event::ButtonPressed(mouse::Button::Left),
                mouse::Event::ButtonReleased(mouse::Button::Left),
            ] {
                view.as_widget_mut().update(
                    &mut tree,
                    &Event::Mouse(event),
                    Layout::new(&node),
                    mouse::Cursor::Available(bounds.center()),
                    &renderer,
                    &mut clipboard::Null,
                    &mut Shell::new(&mut messages),
                    &viewport,
                );
            }
            messages
        };
        assert_eq!(messages.len(), 1);
        app.update(messages.pop().unwrap()).units()
    };

    let confirm_title = "Забыть настройки кеша?";
    let forget = "Забыть настройки кеша";
    assert_eq!(click(&mut app, forget, settings_size), 0);
    assert!(
        layout(&mut app.view(), &renderer, confirm_size)
            .2
            .contains(confirm_title)
    );

    assert_eq!(click(&mut app, "Отмена", confirm_size), 0);
    assert_eq!(click(&mut app, forget, settings_size), 0);
    assert_eq!(app.update(Message::DismissDialog).units(), 0);
    assert!(
        !layout(&mut app.view(), &renderer, settings_size)
            .2
            .contains(confirm_title)
    );
    assert!(
        app.cache
            .as_ref()
            .is_ok_and(|cache| cache.as_ref() == Some(&metadata))
    );

    assert_eq!(click(&mut app, forget, settings_size), 0);
    assert_eq!(click(&mut app, forget, confirm_size), 1);
    let _ = app.update(Message::CacheForgotten(true));
    assert!(app.cache.is_ok_and(|cache| cache.is_none()));
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
    let _ = app.update(Message::CacheSettings(
        cache_settings::Message::ConfirmForget,
    ));
    let _ = app.update(Message::CacheForgotten(true));
    assert!(app.cache.is_ok_and(|cache| cache.is_none()));
    assert!(app.cache_expiration_warning.is_none());
}
