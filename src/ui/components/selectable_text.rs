use iced::{
    Color, Element, Font,
    widget::{
        text,
        text_editor::{self, Action, Binding, Content},
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
    text_editor::TextEditor::new(content)
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
        .into()
}
