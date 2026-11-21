use fluent_bundle::FluentArgs;
use iced::{
    Center, Color, Element, Fill, Font, Padding, Shrink,
    widget::{button, column, container, row, scrollable, text, text_editor},
};
use std::{borrow::Cow, path::Path};

use crate::{
    git,
    i18n::I18n,
    rebuild::{self, Mode},
    ui::style::{buttons, colors, containers, spacing},
};

mod journal;
mod selection;

pub use selection::State;

const CARD_WIDTH: f32 = 560.0;
const OUTPUT_WIDTH: f32 = CARD_WIDTH * 2.0;

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
    Download,
    Rebuild,
    InterruptRebuild,
    ToggleJournal,
    SelectText(selection::Target, text_editor::Action),
}

pub enum DownloadStatus {
    Idle,
    Running,
    Finished(Result<(), git::Error>),
}

impl DownloadStatus {
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }
}

pub struct Repository<'a> {
    pub path: Cow<'a, Path>,
    pub git_available: bool,
    pub rebuild_tools: rebuild::Tools,
    pub working_copy: git::WorkingCopy,
    pub busy: bool,
    pub download: &'a DownloadStatus,
}

pub fn view<'a>(
    i18n: &'a I18n,
    repository: Option<Repository<'a>>,
    mode: Mode,
    loading: bool,
    load_error: bool,
    rebuild: Option<&rebuild::Run>,
    state: &'a State,
) -> Element<'a, Message> {
    let controls = match repository {
        Some(repository) => configured(i18n, repository, mode, state),
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

            container(content).center_x(Fill).into()
        }
    };

    let controls = container(scrollable(controls).height(Shrink))
        .width(Fill)
        .max_width(CARD_WIDTH)
        .max_height(230);
    let has_output = rebuild.is_some_and(rebuild::Run::has_lines);
    let journal = container(journal::view(i18n, rebuild, state))
        .width(Fill)
        .max_width(if has_output { OUTPUT_WIDTH } else { CARD_WIDTH });
    let page = column![controls, journal]
        .width(Fill)
        .height(Fill)
        .spacing(spacing::LG)
        .align_x(Center);

    container(page)
        .padding(Padding {
            bottom: spacing::LG,
            ..spacing::PAGE_PADDING.into()
        })
        .width(Fill)
        .height(Fill)
        .into()
}

fn configured<'a>(
    i18n: &I18n,
    repository: Repository<'a>,
    mode: Mode,
    state: &'a State,
) -> Element<'a, Message> {
    let mut args = FluentArgs::new();
    args.set("path", repository.path.to_string_lossy());
    let mut location = column![
        text(i18n.with_args("overview-repository", &args))
            .wrapping(text::Wrapping::WordOrGlyph)
            .align_x(Center)
    ]
    .spacing(spacing::SM)
    .align_x(Center);
    if repository.working_copy.has_local_changes {
        location = location.push(
            text(i18n.text("overview-local-changes"))
                .size(14)
                .color(colors::WARNING)
                .align_x(Center),
        );
    }
    let modes = [Mode::Test, Mode::Boot, Mode::Switch].into_iter().fold(
        row![].spacing(spacing::SM),
        |row, choice| {
            row.push(
                button(text(i18n.text(choice.label())).font(Font::MONOSPACE))
                    .padding([7, 12])
                    .style(move |_, status| buttons::choice(mode == choice, status))
                    .on_press_maybe((!repository.busy).then_some(Message::SelectMode(choice))),
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
            .style(buttons::primary)
            .on_press_maybe(
                (repository.rebuild_tools.available()
                    && repository.working_copy.matches_revision
                    && !repository.busy)
                    .then_some(Message::Rebuild),
            ),
        button(text(i18n.text("overview-download")))
            .padding([7, 12])
            .style(buttons::secondary)
            .on_press_maybe(
                (repository.git_available && !repository.busy).then_some(Message::Download),
            ),
    ]
    .spacing(spacing::MD);

    let mut content = column![location, modes, hint, actions]
        .spacing(spacing::MD)
        .width(Fill)
        .align_x(Center);

    if !repository.git_available {
        content = content.push(
            text(i18n.text("overview-git-unavailable"))
                .size(14)
                .color(colors::WARNING)
                .align_x(Center),
        );
    }
    for (available, program) in [
        (repository.rebuild_tools.pkexec, rebuild::PKEXEC),
        (
            repository.rebuild_tools.nixos_rebuild,
            rebuild::NIXOS_REBUILD,
        ),
    ] {
        if !available {
            args.set("program", program);
            content = content.push(
                text(i18n.with_args("overview-rebuild-tool-unavailable", &args))
                    .size(14)
                    .color(colors::WARNING)
                    .wrapping(text::Wrapping::WordOrGlyph)
                    .align_x(Center),
            );
        }
    }
    if repository.git_available
        && repository.rebuild_tools.available()
        && !repository.working_copy.matches_revision
        && !repository.busy
    {
        content = content.push(
            text(i18n.text("overview-rebuild-requires-revision"))
                .size(14)
                .color(colors::TEXT_MUTED)
                .align_x(Center),
        );
    }
    if let Some(status) = download_status(i18n, repository.download, state) {
        content = content.push(status);
    }

    container(content)
        .width(Fill)
        .padding(spacing::XL)
        .style(containers::card)
        .into()
}

fn download_status_parts(download: &DownloadStatus) -> Option<(&'static str, Color, Option<&str>)> {
    let (key, color, detail) = match download {
        DownloadStatus::Idle => return None,
        DownloadStatus::Running => ("overview-download-running", colors::INFO, None),
        DownloadStatus::Finished(Ok(())) => ("overview-download-success", colors::SUCCESS, None),
        DownloadStatus::Finished(Err(error)) => {
            let (key, detail) = match error {
                git::Error::Unavailable => ("overview-git-unavailable", None),
                git::Error::Path => ("overview-download-path", None),
                git::Error::NotRepository => ("overview-download-not-repository", None),
                git::Error::Origin => ("overview-download-origin", None),
                git::Error::LocalChanges => ("overview-download-local-changes", None),
                git::Error::Inspect(detail) => ("overview-download-inspect", Some(detail)),
                git::Error::Clone(detail) => ("overview-download-clone", Some(detail)),
                git::Error::Fetch(detail) => ("overview-download-fetch", Some(detail)),
                git::Error::Checkout(detail) => ("overview-download-checkout", Some(detail)),
                git::Error::Revision => ("overview-download-revision", None),
            };
            (key, colors::DANGER, detail)
        }
    };

    Some((key, color, detail.map(String::as_str)))
}

fn download_status<'a>(
    i18n: &I18n,
    download: &DownloadStatus,
    state: &'a State,
) -> Option<Element<'a, Message>> {
    let (key, color, _) = download_status_parts(download)?;
    let status = if matches!(download, DownloadStatus::Finished(Err(_))) {
        selection::view(
            &state.download_error,
            selection::Target::DownloadError,
            Font::DEFAULT,
            14,
            color,
        )
    } else {
        text(i18n.text(key))
            .size(14)
            .color(color)
            .align_x(Center)
            .into()
    };
    let mut content = column![status].spacing(spacing::MD).align_x(Center);
    if !state.download_detail.is_empty() {
        content = content.push(
            container(
                scrollable(selection::view(
                    &state.download_detail,
                    selection::Target::DownloadDetail,
                    Font::MONOSPACE,
                    13,
                    colors::TEXT,
                ))
                .height(Shrink),
            )
            .width(Fill)
            .max_height(88),
        );
    }
    Some(content.into())
}
