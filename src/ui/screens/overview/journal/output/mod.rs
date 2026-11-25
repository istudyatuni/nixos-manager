use std::cell::RefCell;

use iced::{
    Element, Event, Fill, Length, Rectangle, Renderer, Shrink, Size, Theme,
    advanced::{
        Clipboard, Layout, Shell, Widget, clipboard, layout, mouse, renderer,
        widget::{Tree, tree},
    },
    keyboard::{self, Key, key::Named},
};

use super::Message;
use crate::{command_runner::LogLine, ui::style::spacing};

mod log;
mod scroll;

use log::Log;

#[derive(Default)]
pub struct Output {
    log: RefCell<Log>,
    scroll: scroll::Position,
}

impl Output {
    pub fn is_empty(&self) -> bool {
        self.log.borrow().is_empty()
    }

    pub fn text(&self) -> Option<String> {
        self.log.borrow().text()
    }

    pub fn append(&mut self, lines: impl IntoIterator<Item = LogLine>) {
        self.log.get_mut().append(lines);
    }

    pub fn view(&self) -> Element<'_, Message> {
        self.scroll
            .view(Element::new(View { output: self }), &self.log)
    }
}

#[derive(Default)]
struct Interaction {
    focused: bool,
    dragging: bool,
}

struct View<'a> {
    output: &'a Output,
}

impl Widget<Message, Theme, Renderer> for View<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Interaction>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Interaction::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Fill, Shrink)
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let width = limits.max().width;
        let padding = spacing::MD * 2.0;
        let height = self
            .output
            .log
            .borrow_mut()
            .layout((width - padding).max(0.0));
        layout::Node::new(Size::new(width, height + padding))
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.output.log.borrow_mut().draw(
            renderer,
            layout.bounds().shrink(spacing::MD),
            *viewport,
            tree.state.downcast_ref::<Interaction>().focused,
        );
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<Interaction>();
        let bounds = layout.bounds().shrink(spacing::MD);
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(point) = cursor
                    .position_over(bounds)
                    .filter(|&point| viewport.contains(point))
                else {
                    if state.focused {
                        *state = Interaction::default();
                        shell.request_redraw();
                    }
                    return;
                };
                state.focused = true;
                state.dragging = true;
                self.output
                    .log
                    .borrow_mut()
                    .select_at(bounds, *viewport, point, false);
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if state.dragging => {
                let Some(point) = cursor.land().position() else {
                    return;
                };
                self.output
                    .log
                    .borrow_mut()
                    .select_at(bounds, *viewport, point, true);
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.dragging {
                    copy_primary(&self.output.log.borrow(), clipboard);
                }
                state.dragging = false;
                return;
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                physical_key,
                modifiers,
                ..
            }) if state.focused => {
                let mut log = self.output.log.borrow_mut();
                match key.to_latin(*physical_key) {
                    Some('c') if modifiers.command() => {
                        if let Some(text) = log.copy_selection() {
                            clipboard.write(clipboard::Kind::Standard, text);
                        }
                        shell.capture_event();
                        return;
                    }
                    Some('a') if modifiers.command() => {
                        log.select_all();
                        copy_primary(&log, clipboard);
                    }
                    _ if key.as_ref() == Key::Named(Named::Escape) => {
                        *state = Interaction::default();
                    }
                    _ => return,
                }
            }
            _ => return,
        }
        shell.capture_event();
        shell.request_redraw();
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds().shrink(spacing::MD)) && cursor.is_over(*viewport) {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::None
        }
    }
}

fn copy_primary(log: &Log, clipboard: &mut dyn Clipboard) {
    if cfg!(target_os = "linux")
        && let Some(text) = log.copy_selection()
    {
        clipboard.write(clipboard::Kind::Primary, text);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use iced::{Font, Pixels, Point, advanced::renderer::Headless};

    use super::*;

    #[derive(Default)]
    struct PrimaryClipboard(Option<String>);

    impl Clipboard for PrimaryClipboard {
        fn read(&self, _kind: clipboard::Kind) -> Option<String> {
            None
        }

        fn write(&mut self, kind: clipboard::Kind, contents: String) {
            assert_eq!(kind, clipboard::Kind::Primary);
            self.0 = Some(contents);
        }
    }

    #[tokio::test]
    async fn releasing_a_multiline_log_selection_copies_it_to_primary() {
        let renderer = <Renderer as Headless>::new(Font::DEFAULT, Pixels(16.0), Some("tiny-skia"))
            .await
            .expect("software renderer");
        let mut output = Output::default();
        output.append(["first line", "second line"].map(|line| LogLine {
            generation: 0,
            text: line.to_owned(),
            is_command: false,
        }));

        let mut view = output.view();
        let widget = view.as_widget_mut();
        let mut tree = Tree::new(&*widget);
        let size = Size::new(400.0, 100.0);
        let node = widget.layout(&mut tree, &renderer, &layout::Limits::new(Size::ZERO, size));
        let start = Point::new(spacing::MD + 1.0, spacing::MD + 1.0);
        let end = Point::new(200.0, node.size().height - spacing::MD - 1.0);
        let mut clipboard = PrimaryClipboard::default();
        for (event, position) in [
            (mouse::Event::ButtonPressed(mouse::Button::Left), start),
            (mouse::Event::CursorMoved { position: end }, end),
            (mouse::Event::ButtonReleased(mouse::Button::Left), end),
        ] {
            widget.update(
                &mut tree,
                &Event::Mouse(event),
                Layout::new(&node),
                mouse::Cursor::Available(position),
                &renderer,
                &mut clipboard,
                &mut Shell::new(&mut Vec::new()),
                &Rectangle::with_size(size),
            );
        }
        assert_eq!(clipboard.0.as_deref(), Some("first line\nsecond line"));
    }
}
