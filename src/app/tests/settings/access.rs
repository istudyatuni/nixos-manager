use std::time::Duration;

use iced::{
    Event, Renderer, Size,
    advanced::{Layout, Shell, clipboard, mouse},
};

use crate::{
    app::{App, Message},
    git::{RemoteBranchesError, RemoteRefs},
    i18n::LanguagePreference,
    ui::screens::settings::Message as Settings,
};

use super::super::support::{boot_with_cache, headless, layout};

const REPOSITORY: &str = "https://github.com/example/repository";

fn open_repository_settings() -> App {
    let mut app = boot_with_cache(Ok(None));
    app.git_available = true;
    assert_eq!(app.update(Message::OpenSettings).units(), 0);
    assert_eq!(
        app.update(Message::Settings(Settings::RepositoryChanged(
            REPOSITORY.into()
        )))
        .units(),
        0
    );
    app
}

fn click_inputs(app: &App, renderer: &Renderer, size: Size, indices: &[usize]) -> Vec<Message> {
    let mut view = app.view();
    let (mut tree, node, labels) = layout(&mut view, renderer, size);
    let mut messages = Vec::new();
    for &index in indices {
        view.as_widget_mut().update(
            &mut tree,
            &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Layout::new(&node),
            mouse::Cursor::Available(labels.inputs[index].center()),
            renderer,
            &mut clipboard::Null,
            &mut Shell::new(&mut messages),
            &iced::Rectangle::with_size(size),
        );
    }
    messages
}

#[tokio::test]
async fn access_denial_waits_for_token_blur_and_is_replaced_when_the_token_changes() {
    let renderer = headless().await;
    let size = Size::new(800.0, 1000.0);
    let mut app = open_repository_settings();
    let anonymous_hint = "The repository is inaccessible. Check the URL or enter a token";
    let rejected_hint = "GitHub rejected the token. Check it or enter a new one";

    for message in click_inputs(&app, &renderer, size, &[0, 1]) {
        let _ = app.update(message);
    }
    let anonymous_lookup = app.settings_lookup_generation;
    assert!(anonymous_lookup > 0);
    let _ = app.update(Message::SettingsRemoteRefs(
        anonymous_lookup,
        Err(RemoteBranchesError::AccessDenied),
    ));
    assert!(
        !layout(&mut app.view(), &renderer, size)
            .2
            .contains(anonymous_hint)
    );

    for message in click_inputs(&app, &renderer, size, &[1, 2]) {
        let _ = app.update(message);
    }
    assert_eq!(app.settings_lookup_generation, anonymous_lookup);
    let labels = layout(&mut app.view(), &renderer, size).2;
    assert!(labels.bounds(anonymous_hint).y < labels.bounds("GitHub access token").y);

    let _ = app.update(Message::Settings(Settings::TokenFocused));
    let _ = app.update(Message::Settings(Settings::TokenChanged("invalid".into())));
    let _ = app.update(Message::SettingsRemoteRefs(
        anonymous_lookup,
        Err(RemoteBranchesError::AccessDenied),
    ));
    assert!(
        !layout(&mut app.view(), &renderer, size)
            .2
            .contains(anonymous_hint)
    );
    assert_eq!(
        app.update(Message::Settings(Settings::TokenBlurred))
            .units(),
        1
    );
    let authenticated_lookup = app.settings_lookup_generation;
    assert_ne!(authenticated_lookup, anonymous_lookup);
    let _ = app.update(Message::SettingsRemoteRefs(
        authenticated_lookup,
        Err(RemoteBranchesError::Authentication),
    ));
    let labels = layout(&mut app.view(), &renderer, size).2;
    assert!(!labels.contains(anonymous_hint));
    let token = labels.inputs[1];
    assert!(labels.bounds(rejected_hint).y >= token.y + token.height);
    assert!(labels.bounds(rejected_hint).y < labels.bounds("Local path").y);
}

#[tokio::test]
async fn settings_save_waits_for_the_current_access_check() {
    for (result, can_save) in [
        (
            Ok(RemoteRefs {
                branches: None,
                default_branch: None,
            }),
            true,
        ),
        (
            Err(RemoteBranchesError::TimedOut(Duration::from_secs(15))),
            true,
        ),
        (Err(RemoteBranchesError::AccessDenied), false),
    ] {
        let mut app = open_repository_settings();
        let _ = app.update(Message::Settings(Settings::BranchChanged("main".into())));
        assert_eq!(
            app.update(Message::Settings(Settings::RepositoryBlurred))
                .units(),
            2
        );
        let lookup = app.settings_lookup_generation;
        let _ = app.update(Message::Settings(Settings::TokenFocused));
        assert_eq!(app.update(Message::Settings(Settings::Save)).units(), 0);
        assert_eq!(app.settings_lookup_generation, lookup);
        let timed_out = matches!(result, Err(RemoteBranchesError::TimedOut(_)));
        assert_eq!(
            app.update(Message::SettingsRemoteRefs(lookup, result))
                .units(),
            usize::from(can_save)
        );
        if timed_out {
            app.set_language(LanguagePreference::Russian);
            let renderer = headless().await;
            let labels = layout(&mut app.view(), &renderer, Size::new(800.0, 1000.0)).2;
            assert!(labels.text.iter().any(|(text, _)| {
                text.replace(['\u{2068}', '\u{2069}'], "") == "GitHub не ответил за 15 секунд"
            }));
        }
    }
}
