use iced::{Theme, widget::container};

use super::{borders::outline, colors, radius};

pub fn toolbar(_theme: &Theme) -> container::Style {
    container::Style::default().background(colors::BG_DARK)
}

pub fn card(_theme: &Theme) -> container::Style {
    container::Style::default()
        .background(colors::BG_LIGHT)
        .border(outline(colors::BORDER_MUTED, radius::CARD))
}

pub fn journal(_theme: &Theme) -> container::Style {
    container::Style::default().background(colors::BG_DARK)
}
