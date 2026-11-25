mod borders;
pub mod buttons;
pub mod colors;
pub mod containers;
pub mod inputs;
pub mod pick_lists;
mod radius;
pub mod spacing;

use iced::{Theme, theme::Palette};

pub const TOOLTIP_TEXT_SIZE: f32 = 14.0;

pub fn theme() -> Theme {
    Theme::custom(
        "NixOS Update",
        Palette {
            background: colors::BG,
            text: colors::TEXT,
            primary: colors::PRIMARY,
            success: colors::SUCCESS,
            warning: colors::WARNING,
            danger: colors::DANGER,
        },
    )
}
