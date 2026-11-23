use iced::{
    Element, Fill, Font, Padding, Pixels, Size,
    advanced::{
        graphics::text::Paragraph,
        text::{self as paragraph, Paragraph as _, Shaping, Wrapping},
    },
    alignment,
    widget::{Column, button, container, text},
};

use crate::ui::style::{buttons, containers, spacing};

const TEXT_SIZE: Pixels = Pixels(15.0);
const ITEM_PADDING: Padding = Padding {
    top: 7.0,
    right: spacing::LG,
    bottom: 7.0,
    left: spacing::LG,
};

pub fn panel<'a, Message: Clone + 'a, const N: usize>(
    options: [(String, Message); N],
) -> Element<'a, Message> {
    let label_width = options
        .iter()
        .map(|(label, _)| {
            Paragraph::with_text(paragraph::Text {
                content: label.as_str(),
                bounds: Size::INFINITE,
                size: TEXT_SIZE,
                line_height: paragraph::LineHeight::default(),
                font: Font::DEFAULT,
                align_x: paragraph::Alignment::Default,
                align_y: alignment::Vertical::Top,
                shaping: Shaping::Advanced,
                wrapping: Wrapping::None,
            })
            .min_width()
        })
        .fold(0.0, f32::max);
    let items = Column::with_children(options.into_iter().map(|(label, message)| {
        button(
            text(label)
                .size(TEXT_SIZE)
                .font(Font::DEFAULT)
                .shaping(Shaping::Advanced)
                .wrapping(Wrapping::WordOrGlyph),
        )
        .width(Fill)
        .padding(ITEM_PADDING)
        .style(buttons::menu_item)
        .on_press(message)
        .into()
    }))
    .width(label_width.ceil() + ITEM_PADDING.x());
    container(items)
        .padding(spacing::SM)
        .style(containers::card)
        .into()
}
