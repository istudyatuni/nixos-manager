use iced::{
    Center, Element, Fill, Right,
    widget::{button, container, row, space, text},
};

use super::{
    components::{menu, popup},
    style::{buttons, containers, spacing},
};

pub const SETTINGS_GEAR: &str = "⚙\u{fe0e}";
const GEAR_SIZE: f32 = 36.0;
const VERTICAL_PADDING: f32 = 10.0;

pub fn view<'a, Message: Clone + 'a>(
    title: String,
    open_menu: Option<Message>,
    options: Option<[(String, Message); 2]>,
    dismiss: Message,
) -> Element<'a, Message> {
    let menu_open = options.is_some();
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
    let settings_menu = popup::on_click(gear, options.map(menu::panel), dismiss, Right);

    container(
        row![text(title).size(22), space().width(Fill), settings_menu]
            .align_y(Center)
            .spacing(spacing::LG),
    )
    .padding([VERTICAL_PADDING, spacing::PAGE_PADDING])
    .width(Fill)
    .style(containers::toolbar)
    .into()
}
