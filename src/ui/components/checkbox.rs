use iced::{
    Element, Renderer,
    advanced::text::Renderer as _,
    widget::{checkbox, text},
};

use crate::ui::style::{inputs, spacing};

const SIZE: f32 = 16.0;

pub fn view<'a, Message: Clone + 'a>(
    checked: bool,
    label: String,
    on_toggle: Option<impl Fn(bool) -> Message + 'a>,
) -> Element<'a, Message> {
    checkbox(checked)
        .label(label)
        .size(SIZE)
        .spacing(spacing::MD)
        .style(inputs::checkbox_style)
        .icon(checkbox::Icon {
            font: Renderer::ICON_FONT,
            code_point: Renderer::CHECKMARK_ICON,
            size: Some(14.0.into()),
            line_height: 1.0.into(),
            shaping: text::Shaping::Basic,
        })
        .on_toggle_maybe(on_toggle)
        .into()
}
