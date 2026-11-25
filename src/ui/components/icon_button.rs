use iced::{
    Element, Theme,
    widget::{button, container, text, tooltip},
};

use crate::ui::style::{buttons, containers, spacing};

pub const SIZE: f32 = 34.0;

pub fn close<'a, Message: Clone + 'a>(
    label: String,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    view(text("×").size(20), label, on_press, buttons::secondary)
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
        text(label).size(13),
        tooltip::Position::Top,
    )
    .gap(spacing::SM)
    .style(containers::card)
    .into()
}
