use fluent_bundle::FluentArgs;
use iced::{
    Center, Color, Element, Fill, Font, Renderer,
    advanced::text::Renderer as _,
    widget::{column, container, row, text},
};

use super::{Message, State, selection};
use crate::{
    i18n::I18n,
    rebuild::{Error, Run, Step},
    ui::{
        components::{icon::Icon, icon_button},
        style::{buttons, colors, containers, spacing},
    },
};

mod output;

pub(super) use output::Output;

pub fn view<'a>(i18n: &I18n, run: Option<&Run>, state: &'a State) -> Element<'a, Message> {
    let has_journal = state.has_journal();
    let mut content = column![controls(i18n, run, state.journal_expanded, has_journal)]
        .width(Fill)
        .spacing(spacing::MD);

    if let Some(run) = run {
        let status = if has_error(run) {
            selection::view(
                &state.rebuild_error,
                selection::Target::RebuildError,
                Font::DEFAULT,
                14,
                colors::DANGER,
            )
        } else {
            let (message, color) = rebuild_status(i18n, run);
            text(message)
                .size(14)
                .color(color)
                .wrapping(text::Wrapping::WordOrGlyph)
                .into()
        };
        content = content.push(status);
    }
    if state.journal_expanded {
        content = content.push(log(i18n, state));
    }

    container(content)
        .width(Fill)
        .padding(spacing::LG)
        .style(containers::card)
        .into()
}

fn controls<'a>(
    i18n: &I18n,
    run: Option<&Run>,
    expanded: bool,
    has_journal: bool,
) -> Element<'a, Message> {
    let (toggle_label, arrow) = if expanded {
        ("overview-hide-journal", Renderer::SCROLL_UP_ICON)
    } else {
        ("overview-show-journal", Renderer::SCROLL_DOWN_ICON)
    };
    let toggle = icon_button::view(
        text(arrow.to_string()).font(Renderer::ICON_FONT).size(14),
        i18n.text(toggle_label),
        Some(Message::ToggleJournal),
        buttons::secondary,
    );

    let interrupt = run.filter(|run| run.is_running()).map(|run| {
        icon_button::view(
            Icon::Stop,
            i18n.text("overview-rebuild-interrupt"),
            (!run.interrupting).then_some(Message::InterruptRebuild),
            buttons::danger,
        )
    });

    let log_actions = row![
        icon_button::view(
            Icon::Copy,
            i18n.text("overview-copy-journal"),
            has_journal.then_some(Message::CopyJournal),
            buttons::secondary,
        ),
        icon_button::view(
            Icon::Trash,
            i18n.text("overview-clear-journal"),
            has_journal.then_some(Message::ClearJournal),
            buttons::secondary,
        ),
    ]
    .spacing(spacing::MD);

    let journal_heading = row![toggle, text(i18n.text("overview-journal-title")).size(16)]
        .spacing(spacing::LG)
        .align_y(Center);

    row![container(journal_heading).width(Fill)]
        .push(interrupt)
        .push(container(log_actions).align_right(Fill))
        .spacing(spacing::MD)
        .into()
}

fn log<'a>(i18n: &I18n, state: &'a State) -> Element<'a, Message> {
    let log: Element<'_, Message> = if state.has_journal() {
        state.journal.view()
    } else {
        container(
            text(i18n.text("overview-journal-empty"))
                .width(Fill)
                .size(13)
                .color(colors::TEXT_MUTED)
                .wrapping(text::Wrapping::WordOrGlyph),
        )
        .padding(spacing::MD)
        .into()
    };

    container(log).width(Fill).style(containers::journal).into()
}

pub(super) fn has_error(run: &Run) -> bool {
    run.outcome
        .is_some_and(|outcome| matches!(outcome, Err(error) if !matches!(error, Error::Cancelled)))
}

pub(super) fn rebuild_status(i18n: &I18n, run: &Run) -> (String, Color) {
    let mut args = FluentArgs::new();
    let step = match run.step {
        Step::History => "overview-rebuild-step-history",
        Step::Inspect => "overview-rebuild-step-inspect",
        Step::Authorize => "overview-rebuild-step-authorize",
        Step::Build => "overview-rebuild-step-build",
        Step::Register => "overview-rebuild-step-register",
        Step::Boot => "overview-rebuild-step-boot",
        Step::Activate => "overview-rebuild-step-activate",
        Step::BootAndActivate => "overview-rebuild-step-switch",
    };
    args.set("step", i18n.text(step));
    let (key, color) = match run.outcome {
        None if run.interrupting => ("overview-rebuild-interrupting", colors::INFO),
        None => ("overview-rebuild-running", colors::INFO),
        Some(Ok(())) => (
            match (run.steps.boot, run.steps.activate) {
                (false, false) => "overview-rebuild-build-success",
                (false, true) => "overview-rebuild-activate-success",
                (true, false) => "overview-rebuild-boot-success",
                (true, true) => "overview-rebuild-switch-success",
            },
            colors::SUCCESS,
        ),
        Some(Err(Error::Cancelled)) => ("overview-rebuild-cancelled", colors::TEXT_MUTED),
        Some(Err(Error::AuthorizationFailed)) => ("overview-rebuild-authorization", colors::DANGER),
        Some(Err(Error::Exited(code))) => {
            args.set("code", code.to_string());
            ("overview-rebuild-exited", colors::DANGER)
        }
        Some(Err(Error::Interrupted)) => ("overview-rebuild-interrupted", colors::DANGER),
        Some(Err(Error::LaunchFailed)) => ("overview-rebuild-launch-failed", colors::DANGER),
        Some(Err(Error::WaitFailed)) => ("overview-rebuild-wait-failed", colors::DANGER),
        Some(Err(Error::InvalidRepository)) => {
            ("overview-rebuild-invalid-repository", colors::DANGER)
        }
        Some(Err(Error::Unsynced)) => ("overview-rebuild-requires-revision", colors::DANGER),
        Some(Err(Error::MissingHostname)) => ("overview-rebuild-hostname", colors::DANGER),
        Some(Err(Error::InvalidPath)) => ("overview-rebuild-invalid-path", colors::DANGER),
        Some(Err(Error::MissingFlake)) => ("overview-rebuild-missing-flake", colors::DANGER),
        Some(Err(Error::BuildResult)) => ("overview-rebuild-build-result", colors::DANGER),
        Some(Err(Error::BuildOutdated)) => ("overview-rebuild-build-outdated", colors::DANGER),
        Some(Err(Error::History)) => ("overview-rebuild-history-error", colors::DANGER),
        Some(Err(Error::TargetUnavailable)) => {
            ("overview-rebuild-target-unavailable", colors::DANGER)
        }
    };
    (i18n.with_args(key, &args), color)
}
