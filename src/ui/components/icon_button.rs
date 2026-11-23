use iced::{
    Element, Theme,
    widget::{button, container, text, tooltip},
};

use crate::ui::style::{containers, spacing};

pub fn view<'a, Message: Clone + 'a>(
    icon: impl Into<Element<'a, Message>>,
    label: String,
    on_press: Option<Message>,
    style: fn(&Theme, button::Status) -> button::Style,
) -> Element<'a, Message> {
    tooltip(
        button(container(icon).center(34))
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
