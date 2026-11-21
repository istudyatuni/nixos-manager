use fluent_bundle::FluentArgs;
use iced::{
    Center, Color, Element, Fill, Font, Shrink,
    widget::{button, column, container, row, scrollable, space, text},
};

use super::{Message, State, selection};
use crate::{
    i18n::I18n,
    rebuild::{Mode, Outcome, Run},
    ui::style::{buttons, colors, containers, spacing},
};

pub fn view<'a>(i18n: &I18n, run: Option<&Run>, state: &'a State) -> Element<'a, Message> {
    let toggle = if state.journal_expanded {
        "overview-hide-journal"
    } else {
        "overview-show-journal"
    };
    let mut actions = row![
        button(text(i18n.text(toggle)).size(13))
            .padding([5, 8])
            .style(buttons::secondary)
            .on_press(Message::ToggleJournal),
        space().width(Fill),
    ]
    .align_y(Center)
    .spacing(spacing::MD);
    if let Some(run) = run.filter(|run| run.is_running()) {
        actions = actions.push(
            button(text(i18n.text("overview-rebuild-interrupt")).size(13))
                .padding([5, 8])
                .style(buttons::danger)
                .on_press_maybe((!run.interrupting).then_some(Message::InterruptRebuild)),
        );
    }

    let mut content = column![actions].width(Fill).spacing(spacing::MD);

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

fn log<'a>(i18n: &I18n, state: &'a State) -> Element<'a, Message> {
    let output = if state.has_journal() {
        selection::view(
            &state.journal,
            selection::Target::Journal,
            Font::MONOSPACE,
            13,
            colors::TEXT,
        )
    } else {
        text(i18n.text("overview-journal-empty"))
            .width(Fill)
            .size(13)
            .color(colors::TEXT_MUTED)
            .wrapping(text::Wrapping::WordOrGlyph)
            .into()
    };
    let log = scrollable(container(output).padding(spacing::MD))
        .width(Fill)
        .height(Shrink)
        .anchor_bottom();

    container(log).width(Fill).style(containers::journal).into()
}

pub(super) fn has_error(run: &Run) -> bool {
    run.outcome
        .is_some_and(|outcome| !matches!(outcome, Outcome::Success | Outcome::Cancelled))
}

pub(super) fn rebuild_status(i18n: &I18n, run: &Run) -> (String, Color) {
    let mut args = FluentArgs::new();
    args.set("mode", i18n.text(run.mode.label()));
    let (key, color) = match run.outcome {
        None if run.interrupting => ("overview-rebuild-interrupting", colors::INFO),
        None => ("overview-rebuild-running", colors::INFO),
        Some(Outcome::Success) => (
            match run.mode {
                Mode::Test => "overview-rebuild-test-success",
                Mode::Boot => "overview-rebuild-boot-success",
                Mode::Switch => "overview-rebuild-switch-success",
            },
            colors::SUCCESS,
        ),
        Some(Outcome::Cancelled) => ("overview-rebuild-cancelled", colors::TEXT_MUTED),
        Some(Outcome::AuthorizationFailed) => ("overview-rebuild-authorization", colors::DANGER),
        Some(Outcome::Exited(code)) => {
            args.set("code", code.to_string());
            ("overview-rebuild-exited", colors::DANGER)
        }
        Some(Outcome::Interrupted) => ("overview-rebuild-interrupted", colors::DANGER),
        Some(Outcome::LaunchFailed) => ("overview-rebuild-launch-failed", colors::DANGER),
        Some(Outcome::WaitFailed) => ("overview-rebuild-wait-failed", colors::DANGER),
        Some(Outcome::InvalidRepository) => ("overview-rebuild-invalid-repository", colors::DANGER),
        Some(Outcome::Unsynced) => ("overview-rebuild-requires-revision", colors::DANGER),
        Some(Outcome::MissingHostname) => ("overview-rebuild-hostname", colors::DANGER),
        Some(Outcome::InvalidPath) => ("overview-rebuild-invalid-path", colors::DANGER),
        Some(Outcome::MissingFlake) => ("overview-rebuild-missing-flake", colors::DANGER),
    };
    (i18n.with_args(key, &args), color)
}
