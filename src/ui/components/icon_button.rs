use iced::{
    Element, Theme,
    widget::{button, container, text, tooltip},
};

use super::icon;
use crate::ui::style::{TOOLTIP_TEXT_SIZE, buttons, containers, spacing};

pub const SIZE: f32 = 34.0;

pub fn close<'a, Message: Clone + 'a>(
    label: String,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    view(icon::close(), label, on_press, buttons::secondary)
}

pub fn view<'a, Message: Clone + 'a>(
    icon: impl Into<Element<'a, Message>>,
    label: String,
    on_press: Option<Message>,
    style: fn(&Theme, button::Status) -> button::Style,
) -> Element<'a, Message> {
    tooltip(
        button(container(icon).center(SIZE))
            .padding(0)
            .style(style)
            .on_press_maybe(on_press),
        text(label).size(TOOLTIP_TEXT_SIZE),
        tooltip::Position::Top,
    )
    .gap(spacing::SM)
    .style(containers::card)
    .into()
}
