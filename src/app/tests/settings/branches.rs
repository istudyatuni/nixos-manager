use iced::{
    Event, Point, Rectangle, Size, Vector,
    advanced::{Layout, Shell, clipboard, mouse},
};

use crate::{
    app::{App, Message},
    config::Config,
    git,
    ui::{icon, screens::settings},
};

use super::super::support::{boot_with_cache, headless, layout};

#[tokio::test]
async fn settings_open_the_branch_picker_without_requesting_remote_branches() {
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
        let arrow = Point::new(
            branch_input.x + branch_input.width - 12.0,
            branch_input.center_y(),
        );
        view.as_widget_mut().update(
            &mut tree,
            &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Layout::new(&node),
            mouse::Cursor::Available(arrow),
            &renderer,
            &mut clipboard::Null,
            &mut Shell::new(&mut messages),
            &Rectangle::with_size(size),
        );
        messages
    };
    assert!(matches!(
        messages.as_slice(),
        [Message::Settings(settings::Message::InputFocused)]
    ));
    assert_eq!(app.update(messages.into_iter().next().unwrap()).units(), 0);
}

#[tokio::test]
async fn settings_load_github_branches_from_the_button_next_to_the_input() {
    let mut app = boot_with_cache(Ok(None));
    app.config = Some(Config {
        repository: "https://github.com/example/repository".into(),
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
    let viewport = Rectangle::with_size(size);
    let click_load = |app: &App| {
        let mut view = app.view();
        let (mut tree, node, labels) = layout(&mut view, &renderer, size);
        let branch_label = labels.bounds("Branch");
        let branch_input = labels
            .inputs
            .iter()
            .find(|input| input.y > branch_label.y)
            .unwrap();
        let icon = labels.bounds(icon::glyph("cloud_download"));
        let position = icon.center();
        assert!(icon.x > branch_input.x + branch_input.width);
        assert!((position.y - branch_input.center_y()).abs() < 1.0);

        let root = Layout::new(&node);
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
                root,
                mouse::Cursor::Available(position),
                &renderer,
                &mut clipboard::Null,
                &mut Shell::new(&mut messages),
                &viewport,
            );
        }
        assert!(
            widget
                .overlay(&mut tree, root, &renderer, &viewport, Vector::ZERO)
                .is_some()
        );
        messages
    };

    let messages = click_load(&app);
    assert!(matches!(
        messages.as_slice(),
        [Message::Settings(settings::Message::LoadRemoteBranches)]
    ));
    assert_eq!(app.update(messages.into_iter().next().unwrap()).units(), 1);
    assert!(click_load(&app).is_empty());
    assert_eq!(
        app.update(Message::Settings(settings::Message::LoadRemoteBranches))
            .units(),
        0
    );

    let _ = app.update(Message::SettingsRemoteRefs(
        app.settings_lookup_generation,
        Err(git::RemoteBranchesError::Network),
    ));
    let messages = click_load(&app);
    assert_eq!(messages.len(), 1);
    assert_eq!(app.update(messages.into_iter().next().unwrap()).units(), 1);

    let _ = app.update(Message::SettingsRemoteRefs(
        app.settings_lookup_generation,
        Ok(git::RemoteRefs {
            branches: Some(Vec::new()),
            default_branch: Some("main".into()),
        }),
    ));
    assert!(click_load(&app).is_empty());
}
