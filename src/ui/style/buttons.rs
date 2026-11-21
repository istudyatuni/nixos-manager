use iced::{Theme, widget::button};

use super::{borders::outline, colors, radius};

pub fn primary(_theme: &Theme, status: button::Status) -> button::Style {
    outlined(status, colors::PRIMARY, false)
}

pub fn secondary(_theme: &Theme, status: button::Status) -> button::Style {
    outlined(status, colors::TEXT, false)
}

pub fn choice(selected: bool, status: button::Status) -> button::Style {
    let text_color = if selected {
        colors::PRIMARY
    } else {
        colors::TEXT
    };
    outlined(status, text_color, selected)
}

fn outlined(status: button::Status, text_color: iced::Color, selected: bool) -> button::Style {
    let (background, border, text_color) = match status {
        button::Status::Disabled => (
            colors::BG,
            colors::BORDER_MUTED,
            colors::INFO.scale_alpha(0.68),
        ),
        button::Status::Pressed => (colors::BORDER_MUTED, colors::PRIMARY, text_color),
        button::Status::Active | button::Status::Hovered if selected => {
            (colors::BG_LIGHT, colors::PRIMARY, text_color)
        }
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
