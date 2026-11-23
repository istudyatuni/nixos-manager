use iced::{
    Center, Element, Fill, Font, Padding, Pixels, Right, Size,
    advanced::{
        graphics::text::Paragraph,
        text::{self as paragraph, Paragraph as _, Shaping, Wrapping},
    },
    alignment,
    widget::{Column, button, container, mouse_area, opaque, row, space, stack, text},
};

use super::style::{buttons, containers, spacing};

pub const SETTINGS_GEAR: &str = "⚙\u{fe0e}";
const GEAR_SIZE: f32 = 36.0;
const VERTICAL_PADDING: f32 = 10.0;
const MENU_TEXT_SIZE: Pixels = Pixels(15.0);
const MENU_ITEM_PADDING: Padding = Padding {
    top: 7.0,
    right: spacing::LG,
    bottom: 7.0,
    left: spacing::LG,
};

pub fn view<'a, Message: Clone + 'a>(
    title: String,
    menu_open: bool,
    open_menu: Option<Message>,
) -> Element<'a, Message> {
    let gear = button(
        text(SETTINGS_GEAR)
            .size(20)
            .width(Fill)
            .height(Fill)
            .align_x(Center)
            .align_y(Center),
    )
    .width(GEAR_SIZE)
    .height(GEAR_SIZE)
    .padding(0)
    .style(move |theme, status| {
        buttons::secondary(
            theme,
            if menu_open {
                button::Status::Pressed
            } else {
                status
            },
        )
    })
    .on_press_maybe(open_menu);

    container(
        row![text(title).size(22), space().width(Fill), gear]
            .align_y(Center)
            .spacing(spacing::LG),
    )
    .padding([VERTICAL_PADDING, spacing::PAGE_PADDING])
    .width(Fill)
    .style(containers::toolbar)
    .into()
}

pub fn settings_menu<'a, Message: Clone + 'a>(
    options: [(String, Message); 2],
    dismiss: Message,
) -> Element<'a, Message> {
    let label_width = options
        .iter()
        .map(|(label, _)| {
            Paragraph::with_text(paragraph::Text {
                content: label.as_str(),
                bounds: Size::INFINITE,
                size: MENU_TEXT_SIZE,
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

    let menu = Column::with_children(options.into_iter().map(|(label, message)| {
        button(
            text(label)
                .size(MENU_TEXT_SIZE)
                .font(Font::DEFAULT)
                .shaping(Shaping::Advanced)
                .wrapping(Wrapping::WordOrGlyph),
        )
        .width(Fill)
        .padding(MENU_ITEM_PADDING)
        .style(buttons::menu_item)
        .on_press(message)
        .into()
    }))
    .width(label_width.ceil() + MENU_ITEM_PADDING.x());
    let menu = container(menu).padding(spacing::SM).style(containers::card);
    let menu = container(opaque(menu))
        .width(Fill)
        .height(Fill)
        .align_x(Right)
        .padding(Padding {
            top: VERTICAL_PADDING + GEAR_SIZE + spacing::SM,
            right: spacing::PAGE_PADDING,
            bottom: 0.0,
            left: spacing::PAGE_PADDING,
        });
    let backdrop = mouse_area(space().width(Fill).height(Fill)).on_press(dismiss);
    stack![backdrop, menu].into()
}
