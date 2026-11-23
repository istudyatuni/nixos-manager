use iced::{
    Element, Event, Font, Length, Pixels, Point, Rectangle, Renderer, Size, Theme,
    advanced::{
        Clipboard, Layout, Shell, Widget, clipboard, input_method, layout, mouse, renderer,
        text::{self, Paragraph as _, Renderer as TextRenderer},
        widget::{Operation, Tree, operation, tree},
    },
    alignment,
    widget::{TextInput as IcedTextInput, text_input as native},
};

use crate::ui::style::inputs;

const TEXT_SIZE: Pixels = Pixels(15.0);
type Paragraph = <Renderer as TextRenderer>::Paragraph;
pub type State = native::State<Paragraph>;

pub struct TextInput<'a, Message> {
    inner: IcedTextInput<'a, Message>,
    editable: bool,
    secure: bool,
}

pub fn text_input<'a, Message: Clone>(placeholder: &str, value: &str) -> TextInput<'a, Message> {
    TextInput {
        inner: IcedTextInput::new(placeholder, value)
            .size(TEXT_SIZE)
            .padding([8, 10])
            .style(inputs::input),
        editable: false,
        secure: false,
    }
}

impl<'a, Message: Clone> TextInput<'a, Message> {
    pub fn on_input_maybe(mut self, on_input: Option<impl Fn(String) -> Message + 'a>) -> Self {
        self.editable = on_input.is_some();
        self.inner = self.inner.on_input_maybe(on_input);
        self
    }

    pub fn on_submit_maybe(mut self, on_submit: Option<Message>) -> Self {
        self.inner = self.inner.on_submit_maybe(on_submit);
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
        self.inner.tag()
    }

    fn state(&self) -> tree::State {
        self.inner.state()
    }

    fn diff(&self, tree: &mut Tree) {
        self.inner.diff(tree);
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
        Widget::layout(&mut self.inner, tree, renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.inner.operate(tree, layout, renderer, operation);
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
        if cfg!(target_os = "linux")
            && matches!(
                event,
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Middle))
            )
        {
            let state = tree.state.downcast_mut::<State>();
            if self.editable
                && cursor.is_over(*viewport)
                && let Some(position) = cursor.position_over(layout.bounds())
            {
                let bounds = layout
                    .children()
                    .next()
                    .expect("text input layout")
                    .bounds();
                let index = cursor_index(state, bounds, position.x, renderer, self.secure);
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
                if !content.is_empty() {
                    // Let Iced insert the text, update the caret, and publish on_input.
                    let paste = Event::InputMethod(input_method::Event::Commit(content));
                    self.inner.update(
                        tree, &paste, layout, cursor, renderer, clipboard, shell, viewport,
                    );
                }
                return;
            }
            if state.is_focused() {
                state.unfocus();
                shell.request_redraw();
            }
        }

        self.inner.update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
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
            tree,
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
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }
}

impl<'a, Message: Clone + 'a> From<TextInput<'a, Message>> for Element<'a, Message> {
    fn from(input: TextInput<'a, Message>) -> Self {
        Element::new(input)
    }
}

fn cursor_index(
    state: &State,
    bounds: Rectangle,
    x: f32,
    renderer: &Renderer,
    secure: bool,
) -> usize {
    if x <= bounds.x {
        return 0;
    }

    let text = operation::TextInput::text(state);
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
