use iced::{
    Center, Element, Fill, Padding, Right,
    widget::{button, container, mouse_area, opaque, row, space, stack, text},
};

use super::{
    components::menu,
    style::{buttons, containers, spacing},
};

pub const SETTINGS_GEAR: &str = "⚙\u{fe0e}";
const GEAR_SIZE: f32 = 36.0;
const VERTICAL_PADDING: f32 = 10.0;

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
    .style(move |theme, status| buttons::menu_trigger(theme, status, menu_open))
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
    let menu = container(opaque(menu::panel(options)))
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
