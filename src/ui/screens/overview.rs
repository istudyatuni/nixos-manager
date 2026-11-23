use fluent_bundle::FluentArgs;
use iced::{
    Center, Color, Element, Fill, Font, Padding, Shrink,
    widget::{button, checkbox, column, container, responsive, row, scrollable, text, text_editor},
};
use std::{borrow::Cow, path::Path};

use crate::{
    attic::{ExpirationWarning, Metadata},
    git,
    i18n::I18n,
    rebuild,
    ui::{
        components::menu,
        style::{buttons, colors, containers, inputs, spacing},
    },
};

mod journal;
mod selection;

pub use selection::State;

const CARD_WIDTH: f32 = 560.0;
const OUTPUT_WIDTH: f32 = CARD_WIDTH * 2.0;
const COMPACT_WIDTH: f32 = 520.0;
const COMPACT_HEIGHT: f32 = 400.0;
const COMPACT_JOURNAL_HEIGHT: f32 = 224.0;
const ACTIONS_ICON: &str = "☰";

#[derive(Clone)]
pub enum Message {
    SetBoot(bool),
    SetActivate(bool),
    Download,
    Rebuild,
    OpenActionsMenu,
    CloseActionsMenu,
    OpenBuildHistory,
    InterruptRebuild,
    ToggleJournal,
    CopyJournal,
    ClearJournal,
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
    pub working_copy: &'a git::WorkingCopy,
    pub steps: rebuild::Steps,
    pub needs_build: bool,
    pub busy: bool,
    pub download: &'a DownloadStatus,
}

pub enum CacheNotice<'a> {
    LoadError,
    Expiration(&'a Metadata, ExpirationWarning),
}

impl CacheNotice<'_> {
    fn view<'a>(&self, i18n: &I18n) -> Element<'a, Message> {
        let message = match self {
            Self::LoadError => i18n.text("overview-cache-load-error"),
            Self::Expiration(metadata, warning) => {
                let mut args = FluentArgs::new();
                args.set("cache", metadata.cache_name.as_str());
                args.set("date", metadata.expiration_rfc3339());
                let key = match warning {
                    ExpirationWarning::Expired => "overview-cache-expired",
                    ExpirationWarning::Expiring => "overview-cache-expiring",
                };
                i18n.with_args(key, &args)
            }
        };
        container(text(message).size(14).color(colors::WARNING))
            .width(Fill)
            .max_width(CARD_WIDTH)
            .padding(spacing::LG)
            .style(containers::card)
            .into()
    }
}

pub fn view<'a>(
    i18n: &'a I18n,
    repository: Option<Repository<'a>>,
    loading: bool,
    load_error: bool,
    cache_notice: Option<CacheNotice<'a>>,
    rebuild: Option<&'a rebuild::Run>,
    state: &'a State,
) -> Element<'a, Message> {
    responsive(move |size| {
        let compact = size.width < COMPACT_WIDTH || size.height < COMPACT_HEIGHT;
        let controls = controls(
            i18n,
            repository.as_ref(),
            loading,
            load_error,
            rebuild.is_some_and(rebuild::Run::is_running),
            state,
        );
        let controls = container(controls).width(Fill).max_width(CARD_WIDTH);
        let mut journal = container(journal::view(i18n, rebuild, state))
            .width(Fill)
            .max_width(OUTPUT_WIDTH);
        if compact {
            journal = journal.max_height(COMPACT_JOURNAL_HEIGHT);
        }

        let page = container(
            column![]
                .push(cache_notice.as_ref().map(|notice| notice.view(i18n)))
                .push(controls)
                .push(journal)
                .width(Fill)
                .spacing(spacing::LG)
                .align_x(Center),
        )
        .padding(Padding {
            bottom: spacing::LG,
            ..spacing::PAGE_PADDING.into()
        })
        .width(Fill);

        if compact {
            scrollable(page).height(Fill).into()
        } else {
            page.height(Fill).into()
        }
    })
    .into()
}

fn controls<'a>(
    i18n: &I18n,
    repository: Option<&Repository<'_>>,
    loading: bool,
    load_error: bool,
    rebuild_running: bool,
    state: &'a State,
) -> Element<'a, Message> {
    match repository {
        Some(repository) => configured(i18n, repository, state),
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
            if !loading {
                content = content.push(actions_menu(i18n, state, !rebuild_running));
            }

            container(content).center_x(Fill).into()
        }
    }
}

fn actions_menu<'a>(i18n: &I18n, state: &State, enabled: bool) -> Element<'a, Message> {
    let open = state.actions_menu_open && enabled;
    let trigger = button(
        text(ACTIONS_ICON)
            .size(18)
            .width(Fill)
            .height(Fill)
            .align_x(Center)
            .align_y(Center),
    )
    .width(36)
    .height(36)
    .padding(0)
    .style(move |theme, status| buttons::menu_trigger(theme, status, open))
    .on_press_maybe(enabled.then_some(Message::OpenActionsMenu));
    let panel =
        open.then(|| menu::panel([(i18n.text("history-title"), Message::OpenBuildHistory)]));
    menu::anchored(trigger, panel, Message::CloseActionsMenu)
}

fn configured<'a>(
    i18n: &I18n,
    repository: &Repository<'_>,
    state: &'a State,
) -> Element<'a, Message> {
    let mut args = FluentArgs::new();
    args.set("path", repository.path.to_string_lossy());
    let working_copy_color = if repository.working_copy.has_local_changes {
        colors::WARNING
    } else {
        colors::TEXT_MUTED
    };
    let location = column![
        text(i18n.with_args("overview-repository", &args))
            .wrapping(text::Wrapping::WordOrGlyph)
            .align_x(Center),
        state.working_copy.view(working_copy_color, |action| {
            Message::SelectText(selection::Target::WorkingCopy, action)
        }),
        state.pending_checkout.as_ref().map(|label| {
            label.view(colors::TEXT_MUTED, |action| {
                Message::SelectText(selection::Target::PendingCheckout, action)
            })
        }),
    ]
    .spacing(spacing::SM)
    .align_x(Center);
    let steps = repository.steps;
    let needs_build = repository.needs_build;
    let options = row![
        checkbox(steps.boot)
            .label(i18n.text("overview-add-to-boot"))
            .size(16)
            .spacing(spacing::MD)
            .style(inputs::checkbox_style)
            .on_toggle_maybe((!repository.busy).then_some(Message::SetBoot)),
        checkbox(steps.activate)
            .label(i18n.text("overview-activate"))
            .size(16)
            .spacing(spacing::MD)
            .style(inputs::checkbox_style)
            .on_toggle_maybe((!repository.busy).then_some(Message::SetActivate)),
    ]
    .spacing(spacing::XL)
    .align_y(Center)
    .wrap()
    .align_x(Center);
    let tools = repository.rebuild_tools;
    let tools_available = tools.missing(needs_build, steps).next().is_none();
    let action = if needs_build {
        "overview-build"
    } else {
        "overview-apply"
    };
    let download = row![
        actions_menu(i18n, state, !repository.busy),
        button(text(i18n.text("overview-download")).wrapping(text::Wrapping::WordOrGlyph))
            .padding([7, 12])
            .style(buttons::secondary)
            .on_press_maybe(
                (repository.git_available && !repository.busy).then_some(Message::Download),
            ),
    ]
    .spacing(spacing::MD)
    .align_y(Center);
    let actions = row![
        download,
        button(text(i18n.text(action)))
            .padding([7, 12])
            .style(buttons::primary)
            .on_press_maybe(
                (tools_available
                    && (needs_build || steps.applies())
                    && repository.working_copy.matches_revision
                    && !repository.busy)
                    .then_some(Message::Rebuild),
            ),
    ]
    .spacing(spacing::MD)
    .align_y(Center)
    .wrap()
    .align_x(Center);

    let mut content = column![location, options, actions]
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
    for program in tools.missing(needs_build, steps) {
        args.set("program", program);
        content = content.push(
            text(i18n.with_args("overview-rebuild-tool-unavailable", &args))
                .size(14)
                .color(colors::WARNING)
                .wrapping(text::Wrapping::WordOrGlyph)
                .align_x(Center),
        );
    }
    if repository.git_available
        && tools_available
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
