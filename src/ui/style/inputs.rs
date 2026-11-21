use iced::{
    Color, Theme,
    widget::{text_editor, text_input},
};

use super::{borders::outline, colors, radius};

pub fn input(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let border = match status {
        text_input::Status::Active => colors::BORDER,
        text_input::Status::Hovered => colors::HIGHLIGHT,
        text_input::Status::Focused { .. } => colors::PRIMARY,
        text_input::Status::Disabled => colors::BORDER_MUTED,
    };
    let disabled = status == text_input::Status::Disabled;

    text_input::Style {
        background: if disabled {
            colors::BG
        } else {
            colors::BG_LIGHT
        }
        .into(),
        border: outline(border, radius::CONTROL),
        icon: colors::TEXT_MUTED,
        placeholder: colors::TEXT_MUTED,
        value: if disabled {
            colors::TEXT_MUTED
        } else {
            colors::TEXT
        },
        selection: colors::BORDER,
    }
}

pub fn selectable_text(color: Color) -> text_editor::Style {
    text_editor::Style {
        background: Color::TRANSPARENT.into(),
        border: iced::Border::default(),
        placeholder: colors::TEXT_MUTED,
        value: color,
        selection: colors::BORDER,
    }
}
