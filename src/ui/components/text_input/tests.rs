use iced::{
    Element, Event, Font, Pixels, Rectangle, Renderer, Size,
    advanced::{
        Layout, Shell, clipboard, input_method, layout, mouse, renderer::Headless, widget::Tree,
    },
    event,
    keyboard::{self, Modifiers, key},
};

use super::{input_state, text_input};

#[cfg(target_os = "linux")]
#[tokio::test]
async fn keyboard_selection_copies_visible_text_to_primary() {
    #[derive(Default)]
    struct PrimaryClipboard(Option<String>);

    impl clipboard::Clipboard for PrimaryClipboard {
        fn read(&self, kind: clipboard::Kind) -> Option<String> {
            assert_eq!(kind, clipboard::Kind::Primary);
            self.0.clone()
        }

        fn write(&mut self, kind: clipboard::Kind, contents: String) {
            assert_eq!(kind, clipboard::Kind::Primary);
            self.0 = Some(contents);
        }
    }

    let renderer = <Renderer as Headless>::new(Font::DEFAULT, Pixels(16.0), Some("tiny-skia"))
        .await
        .expect("software renderer");
    let viewport = Rectangle::with_size(Size::new(400.0, 60.0));
    let home = keyboard::Key::Named(key::Named::Home);
    for secure in [false, true] {
        let mut view: Element<'_, String> = text_input("", "e\u{301}жX")
            .secure(secure)
            .on_input_maybe(Some(|value| value))
            .into();
        let widget = view.as_widget_mut();
        let mut tree = Tree::new(&*widget);
        input_state(&mut tree).focus();
        input_state(&mut tree).move_cursor_to(2);
        let node = widget.layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, viewport.size()),
        );
        let mut clipboard = PrimaryClipboard::default();
        for event in [
            Event::Keyboard(keyboard::Event::ModifiersChanged(Modifiers::SHIFT)),
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: home.clone(),
                modified_key: home.clone(),
                physical_key: key::Physical::Code(key::Code::Home),
                location: keyboard::Location::Standard,
                modifiers: Modifiers::SHIFT,
                text: None,
                repeat: false,
            }),
        ] {
            widget.update(
                &mut tree,
                &event,
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut clipboard,
                &mut Shell::new(&mut Vec::new()),
                &viewport,
            );
        }
        assert_eq!(clipboard.0.as_deref(), (!secure).then_some("e\u{301}ж"));
    }
}

#[tokio::test]
async fn undo_and_redo_restore_text_and_selection_in_a_masked_input() {
    let renderer = <Renderer as Headless>::new(Font::DEFAULT, Pixels(16.0), Some("tiny-skia"))
        .await
        .expect("software renderer");
    let viewport = Rectangle::with_size(Size::new(400.0, 60.0));
    let view = |value: &str| -> Element<'static, String> {
        text_input("Token", value)
            .secure(true)
            .on_input_maybe(Some(|value| value))
            .into()
    };
    let mut tree = Tree::new(view("éx").as_widget());
    input_state(&mut tree).focus();
    input_state(&mut tree).select_range(2, 0);

    let mut dispatch = |value: &str, event: &Event| {
        let mut view = view(value);
        let widget = view.as_widget_mut();
        tree.diff(&*widget);
        let node = widget.layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, viewport.size()),
        );
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        widget.update(
            &mut tree,
            event,
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &renderer,
            &mut clipboard::Null,
            &mut shell,
            &viewport,
        );
        let status = shell.event_status();
        assert!(messages.len() <= 1);
        (messages.pop(), status)
    };

    let shortcut = |character: &str, code, modifiers| {
        let key = keyboard::Key::Character(character.into());
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: key::Physical::Code(code),
            location: keyboard::Location::Standard,
            modifiers: Modifiers::CTRL | modifiers,
            text: None,
            repeat: false,
        })
    };
    let undo = shortcut("я", key::Code::KeyZ, Modifiers::NONE);
    let redo_shift = shortcut("Я", key::Code::KeyZ, Modifiers::SHIFT);
    let redo_y = shortcut("y", key::Code::KeyY, Modifiers::NONE);
    let commit = |value: &str| Event::InputMethod(input_method::Event::Commit(value.into()));

    for (before, event, after) in [
        ("éx", &commit("new"), Some("new")),
        ("new", &undo, Some("éx")),
        ("éx", &redo_shift, Some("new")),
        ("new", &undo, Some("éx")),
        ("éx", &redo_y, Some("new")),
        ("new", &undo, Some("éx")),
        ("éx", &commit("other"), Some("other")),
        ("other", &redo_y, None),
        ("", &undo, None), // The form cleared the input.
        ("", &commit("first"), Some("first")),
        ("first", &undo, Some("")),
        ("", &redo_y, Some("first")),
    ] {
        let (value, status) = dispatch(before, event);
        assert_eq!(value.as_deref(), after);
        assert_eq!(status, event::Status::Captured);
    }

    let alt_undo = shortcut("z", key::Code::KeyZ, Modifiers::ALT);
    assert_eq!(dispatch("first", &alt_undo), (None, event::Status::Ignored));
    let super_undo = shortcut("z", key::Code::KeyZ, Modifiers::LOGO);
    assert_eq!(
        dispatch("first", &super_undo),
        (Some(String::new()), event::Status::Captured)
    );
}
