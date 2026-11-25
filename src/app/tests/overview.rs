use iced::{
    Event, Point, Rectangle, Size, Vector,
    advanced::{Layout, Shell, clipboard, mouse},
};

use crate::{
    app::Message,
    attic,
    config::Config,
    git,
    i18n::LanguagePreference,
    rebuild,
    ui::{
        icon,
        screens::{Screen, cache_settings, overview},
    },
};

use super::support::{boot_with_cache, headless, layout};

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

#[test]
fn journal_collapses_after_successful_cache_connection() {
    use cache_settings::Message as CacheMessage;

    let mut app = boot_with_cache(Ok(None));
    app.screen = Screen::CacheSettings(Box::new(cache_settings::State::new(None, true)));
    let expiration = attic::now() + chrono::TimeDelta::days(2);
    let token = attic::tests::jwt(
        expiration.timestamp(),
        serde_json::json!({ "main": { "r": 1 } }),
    );
    for message in [
        CacheMessage::UrlChanged("https://cache.example.org".into()),
        CacheMessage::TokenChanged(token),
        CacheMessage::CacheSelected("main".into()),
    ] {
        let _ = app.update(Message::CacheSettings(message));
    }
    assert!(!app.overview.journal_expanded);

    for result in [
        Err(attic::Failure {
            stage: attic::Stage::ApplyCache,
            kind: attic::FailureKind::ConfigWrite,
        }),
        Ok(attic::Metadata {
            cache_name: "main".into(),
            cache_expiration: expiration,
            server_url: "https://cache.example.org/".into(),
            server_name: attic::DEFAULT_SERVER_NAME.into(),
        }),
    ] {
        assert_eq!(
            app.update(Message::CacheSettings(CacheMessage::Connect))
                .units(),
            1
        );
        assert!(app.overview.journal_expanded);
        let failed = result.is_err();
        let _ = app.update(Message::CacheConnected(result));
        assert_eq!(app.overview.journal_expanded, failed);
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
        let chevron = labels.bounds(icon::glyph(if expanded {
            "chevron_up"
        } else {
            "chevron_down"
        }));
        let chevron_offset = chevron.center_y() - toggle.center_y();
        let expected_offset = if expanded { 0.0 } else { 1.0 };

        assert!((chevron_offset - expected_offset).abs() < 0.5);
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
async fn clicking_the_checkmark_toggles_the_checkbox() {
    let mut app = boot_with_cache(Ok(None));
    app.config = Some(Config {
        repository: "https://github.com/example/config".into(),
        ..Config::default()
    });
    let renderer = headless().await;
    let size = Size::new(800.0, 540.0);
    let mut view = app.view();
    let (mut tree, node, labels) = layout(&mut view, &renderer, size);
    let checkbox = labels.bounds(&app.i18n.text("overview-activate"));
    let center = Point::new(checkbox.x + 8.0, checkbox.center_y());
    let viewport = Rectangle::with_size(size);
    let mut messages = Vec::new();
    view.as_widget_mut().update(
        &mut tree,
        &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        Layout::new(&node),
        mouse::Cursor::Available(center),
        &renderer,
        &mut clipboard::Null,
        &mut Shell::new(&mut messages),
        &viewport,
    );
    assert!(matches!(
        messages.as_slice(),
        [Message::Overview(overview::Message::SetActivate(false))]
    ));
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
    let trigger = labels.bounds(icon::glyph("menu"));
    let download = labels.bounds(&app.i18n.text("overview-download"));
    assert!(trigger.x + trigger.width <= download.x);
    assert!(trigger.y <= download.center().y && download.center().y <= trigger.y + trigger.height);
}
