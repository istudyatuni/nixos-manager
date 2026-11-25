use iced::{
    Color, Theme,
    widget::{checkbox, text_editor, text_input},
};

use super::{borders::outline, colors, radius};

pub fn checkbox_style(_theme: &Theme, status: checkbox::Status) -> checkbox::Style {
    let (checked, background, border, text_color) = match status {
        checkbox::Status::Active { is_checked } => {
            (is_checked, colors::BG, colors::BORDER, colors::TEXT)
        }
        checkbox::Status::Hovered { is_checked } => (
            is_checked,
            colors::BG_LIGHT,
            colors::HIGHLIGHT,
            colors::TEXT,
        ),
        checkbox::Status::Disabled { is_checked } => (
            is_checked,
            colors::BG,
            colors::BORDER_MUTED,
            colors::TEXT_MUTED,
        ),
    };
    let accent = if matches!(status, checkbox::Status::Disabled { .. }) {
        colors::HIGHLIGHT
    } else {
        colors::PRIMARY
    };
    checkbox::Style {
        background: (if checked { accent } else { background }).into(),
        border: outline(if checked { accent } else { border }, radius::CHECKBOX),
        icon_color: colors::BG,
        text_color: Some(text_color),
    }
}

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

pub fn invalid_input(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let mut style = input(theme, status);
    style.border.color = colors::DANGER;
    style
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
