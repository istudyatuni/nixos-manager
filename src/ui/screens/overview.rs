use fluent_bundle::FluentArgs;
use iced::{
    Center, Element, Fill, Font,
    widget::{button, column, container, row, text},
};
use std::path::Path;

use crate::{
    i18n::I18n,
    ui::style::{buttons, colors, containers, spacing},
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Test,
    Boot,
    Switch,
}

impl Mode {
    fn label(self) -> &'static str {
        match self {
            Self::Test => "overview-mode-test",
            Self::Boot => "overview-mode-boot",
            Self::Switch => "overview-mode-switch",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Test => "overview-mode-test-hint",
            Self::Boot => "overview-mode-boot-hint",
            Self::Switch => "overview-mode-switch-hint",
        }
    }
}

#[derive(Clone)]
pub enum Message {
    SelectMode(Mode),
}

pub fn view<'a>(
    i18n: &I18n,
    path: Option<&Path>,
    mode: Mode,
    loading: bool,
    load_error: bool,
) -> Element<'a, Message> {
    let content = match path {
        Some(path) => configured(i18n, path, mode),
        None => {
            let prompt = if loading {
                "app-loading-settings"
            } else {
                "overview-unconfigured"
            };
            let mut content = column![text(i18n.text(prompt)).align_x(Center)]
                .spacing(spacing::LG)
                .align_x(Center);

            if load_error {
                content = content.push(
                    text(i18n.text("app-settings-load-error"))
                        .size(14)
                        .color(colors::DANGER)
                        .align_x(Center),
                );
            }

            container(content).max_width(520).into()
        }
    };

    container(content)
        .padding(spacing::PAGE_PADDING)
        .center(Fill)
        .into()
}

fn configured<'a>(i18n: &I18n, path: &Path, mode: Mode) -> Element<'a, Message> {
    let mut args = FluentArgs::new();
    args.set("path", path.to_string_lossy());
    let repository = text(i18n.with_args("overview-repository", &args))
        .wrapping(text::Wrapping::WordOrGlyph)
        .align_x(Center);
    let modes = [Mode::Test, Mode::Boot, Mode::Switch].into_iter().fold(
        row![].spacing(spacing::SM),
        |row, choice| {
            row.push(
                button(text(i18n.text(choice.label())).font(Font::MONOSPACE))
                    .padding([7, 12])
                    .style(move |_, status| buttons::choice(mode == choice, status))
                    .on_press(Message::SelectMode(choice)),
            )
        },
    );
    let hint = text(i18n.text(mode.hint()))
        .size(14)
        .color(colors::TEXT_MUTED)
        .align_x(Center);
    let actions = row![
        button(text(i18n.text("overview-update")))
            .padding([7, 12])
            .style(buttons::primary),
        button(text(i18n.text("overview-download")))
            .padding([7, 12])
            .style(buttons::secondary),
    ]
    .spacing(spacing::MD);

    let content = column![repository, modes, hint, actions]
        .spacing(spacing::XL)
        .width(Fill)
        .align_x(Center);

    container(content)
        .width(Fill)
        .max_width(560)
        .padding(spacing::XL)
        .style(containers::card)
        .into()
}
