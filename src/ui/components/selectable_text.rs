use iced::{
    Color, Element, Font, Pixels, Size,
    advanced::{
        graphics::text::Paragraph,
        text::{self as advanced_text, Paragraph as _, highlighter::PlainText},
    },
    alignment,
    widget::{
        text,
        text_editor::{Action, Binding, Content, TextEditor},
    },
};

use crate::ui::style::inputs;

pub fn view<'a, Message: Clone + 'a>(
    content: &'a Content,
    font: Font,
    size: u32,
    color: Color,
    on_action: impl Fn(Action) -> Message + 'a,
) -> Element<'a, Message> {
    editor(content, font, size, color, on_action).into()
}

pub fn perform(content: &mut Content, action: Action) -> Option<String> {
    if action.is_edit() {
        return None;
    }

    let selects_text = matches!(
        action,
        Action::Select(_)
            | Action::SelectWord
            | Action::SelectLine
            | Action::SelectAll
            | Action::Drag(_)
    );
    content.perform(action);

    if cfg!(target_os = "linux") && selects_text {
        content.selection().filter(|text| !text.is_empty())
    } else {
        None
    }
}

/// A selectable label that shrinks to its text so it can be centered by its parent.
pub struct Label {
    content: Content,
    size: u32,
    width: f32,
}

impl Label {
    pub fn new(size: u32) -> Self {
        Self {
            content: Content::new(),
            size,
            width: 0.0,
        }
    }

    pub fn set_text(&mut self, text: &str) {
        if self.content.text() == text {
            return;
        }
        self.content = Content::with_text(text);
        self.width = Paragraph::with_text(advanced_text::Text {
            content: text,
            bounds: Size::new(f32::INFINITY, f32::INFINITY),
            size: Pixels(self.size as f32),
            line_height: advanced_text::LineHeight::default(),
            font: Font::DEFAULT,
            align_x: advanced_text::Alignment::Default,
            align_y: alignment::Vertical::Top,
            shaping: advanced_text::Shaping::Advanced,
            wrapping: advanced_text::Wrapping::None,
        })
        .min_width()
        .ceil();
    }

    pub fn perform(&mut self, action: Action) -> Option<String> {
        perform(&mut self.content, action)
    }

    pub fn view<'a, Message: Clone + 'a>(
        &'a self,
        color: Color,
        on_action: impl Fn(Action) -> Message + 'a,
    ) -> Element<'a, Message> {
        editor(&self.content, Font::DEFAULT, self.size, color, on_action)
            .width(self.width)
            .into()
    }
}

fn editor<'a, Message: Clone + 'a>(
    content: &'a Content,
    font: Font,
    size: u32,
    color: Color,
    on_action: impl Fn(Action) -> Message + 'a,
) -> TextEditor<'a, PlainText, Message> {
    TextEditor::new(content)
        .font(font)
        .size(size)
        .padding(0)
        .wrapping(text::Wrapping::WordOrGlyph)
        .style(move |_, _| inputs::selectable_text(color))
        .on_action(on_action)
        .key_binding(|keypress| {
            Binding::from_key_press(keypress).filter(|binding| {
                matches!(
                    binding,
                    Binding::Copy
                        | Binding::Move(_)
                        | Binding::Select(_)
                        | Binding::SelectAll
                        | Binding::Unfocus
                )
            })
        })
}
