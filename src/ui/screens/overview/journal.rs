use fluent_bundle::FluentArgs;
use iced::{
    Center, Color, Element, Fill, Font, Shrink,
    widget::{button, column, container, row, scrollable, space, text},
};

use super::Message;
use crate::{
    i18n::I18n,
    rebuild::{Mode, Outcome, Run},
    ui::style::{buttons, colors, containers, spacing},
};

pub fn view<'a>(i18n: &I18n, run: Option<&Run>, expanded: bool) -> Element<'a, Message> {
    let toggle = if expanded {
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
        button(text(i18n.text("overview-copy-journal")).size(13))
            .padding([5, 8])
            .style(buttons::secondary)
            .on_press_maybe(
                run.is_some_and(Run::has_lines)
                    .then_some(Message::CopyJournal)
            ),
    ]
    .align_y(Center)
    .spacing(spacing::MD);
    let status = run.map(|run| rebuild_status(i18n, run));
    if let Some((message, _)) = &status
        && run
            .and_then(|run| run.outcome)
            .is_some_and(|outcome| !matches!(outcome, Outcome::Success | Outcome::Cancelled))
    {
        actions = actions.push(
            button(text(i18n.text("overview-copy-error")).size(13))
                .padding([5, 8])
                .style(buttons::secondary)
                .on_press(Message::CopyError(message.clone())),
        );
    }

    let mut content = column![actions].width(Fill).spacing(spacing::MD);
    if let Some((message, color)) = status {
        content = content.push(
            text(message)
                .size(14)
                .color(color)
                .wrapping(text::Wrapping::WordOrGlyph),
        );
    }
    if expanded {
        content = content.push(log(i18n, run));
    }

    container(content)
        .width(Fill)
        .padding(spacing::LG)
        .style(containers::card)
        .into()
}

fn log<'a>(i18n: &I18n, run: Option<&Run>) -> Element<'a, Message> {
    let output = if let Some(run) = run.filter(|run| run.has_lines()) {
        let mut output = run.journal();
        if output.ends_with('\n') {
            // Give the renderer content on the final blank line so it has a height.
            output.push('\u{200b}');
        }
        text(output).font(Font::MONOSPACE)
    } else {
        text(i18n.text("overview-journal-empty")).color(colors::TEXT_MUTED)
    };
    let log = scrollable(
        container(
            output
                .width(Fill)
                .size(13)
                .wrapping(text::Wrapping::WordOrGlyph),
        )
        .padding(spacing::MD),
    )
    .width(Fill)
    .height(Shrink)
    .anchor_bottom();

    container(log).width(Fill).style(containers::journal).into()
}

fn rebuild_status(i18n: &I18n, run: &Run) -> (String, Color) {
    let mut args = FluentArgs::new();
    args.set("mode", i18n.text(run.mode.label()));
    let (key, color) = match run.outcome {
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
