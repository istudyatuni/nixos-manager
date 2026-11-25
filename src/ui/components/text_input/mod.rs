use std::cell::RefCell;

use iced::{
    Element, Event, Font, Length, Pixels, Point, Rectangle, Renderer, Size, Theme,
    advanced::{
        Clipboard, Layout, Shell, Widget, clipboard, input_method, layout, mouse, renderer,
        text::{self, Paragraph as _, Renderer as TextRenderer},
        widget::{Operation, Tree, tree},
    },
    alignment, keyboard,
    widget::{TextInput as IcedTextInput, text_input as native},
};

use crate::ui::style::inputs;

mod history;
#[cfg(test)]
mod tests;

use history::{Action as HistoryAction, History};

const TEXT_SIZE: Pixels = Pixels(15.0);
type Paragraph = <Renderer as TextRenderer>::Paragraph;
type InputState = native::State<Paragraph>;

pub struct TextInput<'a, Message> {
    inner: IcedTextInput<'a, InputMessage>,
    value: String,
    on_input: Option<Box<dyn Fn(String) -> Message + 'a>>,
    on_submit: Option<Message>,
    secure: bool,
}

#[derive(Clone)]
enum InputMessage {
    Changed(String),
    Submit,
}

pub fn text_input<'a, Message: Clone>(placeholder: &str, value: &str) -> TextInput<'a, Message> {
    TextInput {
        inner: IcedTextInput::new(placeholder, value)
            .size(TEXT_SIZE)
            .padding([8, 10])
            .style(inputs::input),
        value: value.to_owned(),
        on_input: None,
        on_submit: None,
        secure: false,
    }
}

pub fn input_state(tree: &mut Tree) -> &mut InputState {
    tree.children[0].state.downcast_mut()
}

pub fn record_edit(tree: &mut Tree, value: &str) {
    let cursor = input_state(tree).cursor();
    tree.state.downcast_mut::<History>().record(value, cursor);
}

impl<'a, Message: Clone> TextInput<'a, Message> {
    pub fn on_input_maybe(mut self, on_input: Option<impl Fn(String) -> Message + 'a>) -> Self {
        self.inner = self
            .inner
            .on_input_maybe(on_input.as_ref().map(|_| InputMessage::Changed));
        self.on_input = on_input.map(|callback| Box::new(callback) as _);
        self
    }

    pub fn on_submit_maybe(mut self, on_submit: Option<Message>) -> Self {
        self.inner = self
            .inner
            .on_submit_maybe(on_submit.as_ref().map(|_| InputMessage::Submit));
        self.on_submit = on_submit;
        self
    }

    pub fn secure(mut self, secure: bool) -> Self {
        self.secure = secure;
        self.inner = self.inner.secure(secure);
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.inner = self.inner.width(width);
        self
    }

    pub fn icon(mut self, icon: native::Icon<Font>) -> Self {
        self.inner = self.inner.icon(icon);
        self
    }
}

impl<Message: Clone> Widget<Message, Theme, Renderer> for TextInput<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<History>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(History::new(self.value.clone()))
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.inner as &dyn Widget<_, _, _>)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.children[0].diff(&self.inner as &dyn Widget<_, _, _>);
        let history = tree.state.downcast_mut::<History>();
        if history.value != self.value {
            // A form reset or automatic value change starts a new editing history.
            *history = History::new(self.value.clone());
        }
    }

    fn size(&self) -> Size<Length> {
        Widget::size(&self.inner)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        Widget::layout(&mut self.inner, &mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.inner
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let history = tree.state.downcast_mut::<History>();
        let inner = &mut tree.children[0];
        let state = inner.state.downcast_mut::<InputState>();
        let mut commit = None;
        let mut restored_cursor = None;

        if self.on_input.is_some()
            && state.is_focused()
            && let Some(action) = HistoryAction::from_event(event)
        {
            shell.capture_event();
            let Some(cursor) = history.restore(action, state.cursor()) else {
                return;
            };
            restored_cursor = Some(cursor.state(&native::Value::new(&history.value)));
            state.select_all();
            commit = Some(history.value.clone());
        } else if cfg!(target_os = "linux")
            && matches!(
                event,
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Middle))
            )
        {
            if self.on_input.is_some()
                && cursor.is_over(*viewport)
                && let Some(position) = cursor.position_over(layout.bounds())
            {
                let bounds = layout
                    .children()
                    .next()
                    .expect("text input layout")
                    .bounds();
                let index = cursor_index(
                    state,
                    &history.value,
                    bounds,
                    position.x,
                    renderer,
                    self.secure,
                );
                state.focus();
                state.move_cursor_to(index);
                shell.capture_event();
                shell.request_redraw();

                let content: String = clipboard
                    .read(clipboard::Kind::Primary)
                    .unwrap_or_default()
                    .chars()
                    .filter(|character| !character.is_control())
                    .collect();
                if content.is_empty() {
                    return;
                }
                commit = Some(content);
            } else if state.is_focused() {
                state.unfocus();
                shell.request_redraw();
            }
        }

        // Use Iced's editing path for both restoring a snapshot and pasting primary selection.
        let commit = commit.map(|value| Event::InputMethod(input_method::Event::Commit(value)));
        let cursor_before = state.cursor();
        let mut messages = Vec::new();
        let mut input_shell = Shell::new(&mut messages);
        self.inner.update(
            inner,
            commit.as_ref().unwrap_or(event),
            layout,
            cursor,
            renderer,
            clipboard,
            &mut input_shell,
            viewport,
        );
        if let Some(cursor) = restored_cursor {
            let state = inner.state.downcast_mut::<InputState>();
            match cursor {
                native::cursor::State::Index(index) => state.move_cursor_to(index),
                native::cursor::State::Selection { start, end } => state.select_range(start, end),
            }
            shell.request_redraw();
        } else if cfg!(target_os = "linux")
            && !self.secure
            && commit.is_none()
            && input_shell.is_event_captured()
        {
            let cursor = inner.state.downcast_ref::<InputState>().cursor();
            let select_all = matches!(
                event,
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key, physical_key, modifiers, ..
                }) if modifiers.control() && key.to_latin(*physical_key) == Some('a')
            );
            if cursor != cursor_before || select_all {
                let value = native::Value::new(&history.value);
                if let Some((start, end)) = cursor.selection(&value) {
                    clipboard.write(
                        clipboard::Kind::Primary,
                        value.select(start, end).to_string(),
                    );
                }
            }
        }
        let history = RefCell::new(history);
        shell.merge(input_shell, |message| match message {
            InputMessage::Changed(value) => {
                history.borrow_mut().record(&value, cursor_before);
                self.on_input.as_ref().expect("editable text input")(value)
            }
            InputMessage::Submit => self.on_submit.clone().expect("text input submit message"),
        });
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        Widget::draw(
            &self.inner,
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.inner
            .mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }
}

impl<'a, Message: Clone + 'a> From<TextInput<'a, Message>> for Element<'a, Message> {
    fn from(input: TextInput<'a, Message>) -> Self {
        Element::new(input)
    }
}

fn cursor_index(
    state: &InputState,
    text: &str,
    bounds: Rectangle,
    x: f32,
    renderer: &Renderer,
    secure: bool,
) -> usize {
    if x <= bounds.x {
        return 0;
    }

    let value = native::Value::new(text);
    let masked = secure.then(|| value.secure().to_string());
    let text = masked.as_deref().unwrap_or(text);
    // Match Iced's single-line shaping and caret-based horizontal scroll.
    let paragraph = Paragraph::with_text(text::Text {
        content: text,
        bounds: Size::new(f32::INFINITY, bounds.height),
        size: TEXT_SIZE,
        line_height: text::LineHeight::default(),
        font: renderer.default_font(),
        align_x: text::Alignment::Default,
        align_y: alignment::Vertical::Center,
        shaping: text::Shaping::Advanced,
        wrapping: text::Wrapping::default(),
    });
    let mut offset = 0.0;
    if state.is_focused() {
        use native::cursor::State::{Index, Selection};
        let index = match state.cursor().state(&value) {
            Index(index) => index,
            Selection { end, .. } => end,
        };
        if let Some(position) = paragraph.grapheme_position(0, index) {
            offset = (position.x + 5.0 - bounds.width).max(0.0);
        }
    }
    paragraph
        .hit_test(Point::new(x - bounds.x + offset, bounds.height / 2.0))
        .and_then(|hit| text.get(..hit.cursor()))
        .map_or(0, |prefix| native::Value::new(prefix).len())
}
