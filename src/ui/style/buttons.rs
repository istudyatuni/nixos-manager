use iced::{Theme, widget::button};

use super::{borders::outline, colors, radius};

pub fn primary(_theme: &Theme, status: button::Status) -> button::Style {
    outlined(status, colors::PRIMARY)
}

pub fn secondary(_theme: &Theme, status: button::Status) -> button::Style {
    outlined(status, colors::TEXT)
}

pub fn menu_trigger(theme: &Theme, status: button::Status, open: bool) -> button::Style {
    let status = if open {
        button::Status::Pressed
    } else {
        status
    };
    secondary(theme, status)
}

pub fn danger(_theme: &Theme, status: button::Status) -> button::Style {
    let mut style = outlined(status, colors::DANGER);
    if status != button::Status::Disabled {
        style.border.color = colors::DANGER.scale_alpha(0.65);
    }
    style
}

pub fn menu_item(_theme: &Theme, status: button::Status) -> button::Style {
    let selected = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: selected.then_some(colors::BORDER_MUTED.into()),
        border: iced::Border::default().rounded(radius::CONTROL),
        text_color: colors::TEXT,
        ..button::Style::default()
    }
}

fn outlined(status: button::Status, text_color: iced::Color) -> button::Style {
    let (background, border, text_color) = match status {
        button::Status::Disabled => (
            colors::BG,
            colors::BORDER_MUTED,
            colors::INFO.scale_alpha(0.68),
        ),
        button::Status::Pressed => (colors::BORDER_MUTED, colors::PRIMARY, text_color),
        button::Status::Hovered => (colors::BG_LIGHT, colors::HIGHLIGHT, text_color),
        button::Status::Active => (colors::BG, colors::BORDER, text_color),
    };

    button::Style {
        background: Some(background.into()),
        border: outline(border, radius::CONTROL),
        text_color,
        ..button::Style::default()
    }
}
