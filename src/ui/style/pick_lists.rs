use iced::{
    Theme,
    widget::{overlay::menu, pick_list},
};

use super::{borders::outline, colors, radius};

pub fn field(_theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let border = match status {
        pick_list::Status::Active => colors::BORDER,
        pick_list::Status::Hovered => colors::HIGHLIGHT,
        pick_list::Status::Opened { .. } => colors::PRIMARY,
    };

    pick_list::Style {
        text_color: colors::TEXT,
        placeholder_color: colors::TEXT_MUTED,
        handle_color: colors::TEXT_MUTED,
        background: colors::BG_LIGHT.into(),
        border: outline(border, radius::CONTROL),
    }
}

pub fn menu(_theme: &Theme) -> menu::Style {
    menu::Style {
        background: colors::BG_LIGHT.into(),
        border: outline(colors::BORDER, radius::CONTROL),
        text_color: colors::TEXT,
        selected_text_color: colors::TEXT,
        selected_background: colors::BORDER_MUTED.into(),
        shadow: Default::default(),
    }
}
