use fluent_bundle::FluentArgs;
use iced::{
    Center, Element, Fill, Font, Shrink,
    widget::{button, column, container, row, scrollable, space, text},
};

use super::Message;
use crate::{
    i18n::I18n,
    rebuild::{Mode, Outcome, Run},
    ui::style::{buttons, colors, containers, spacing},
};

pub fn view<'a>(i18n: &I18n, run: &Run, expanded: bool) -> Element<'a, Message> {
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
    };

    let status = i18n.with_args(key, &args);
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
            .on_press_maybe(run.has_lines().then_some(Message::CopyJournal)),
    ]
    .align_y(Center)
    .spacing(spacing::MD);
    if run
        .outcome
        .is_some_and(|outcome| !matches!(outcome, Outcome::Success | Outcome::Cancelled))
    {
        actions = actions.push(
            button(text(i18n.text("overview-copy-error")).size(13))
                .padding([5, 8])
                .style(buttons::secondary)
                .on_press(Message::CopyError(status.clone())),
        );
    }

    let mut content = column![
        actions,
        text(status)
            .size(14)
            .color(color)
            .wrapping(text::Wrapping::WordOrGlyph),
    ]
    .width(Fill)
    .spacing(spacing::MD);
    if expanded {
        content = content.push(log(run));
    }

    container(content)
        .width(Fill)
        .padding(spacing::LG)
        .style(containers::card)
        .into()
}

fn log<'a>(run: &Run) -> Element<'a, Message> {
    let mut output = run.journal();
    if output.ends_with('\n') {
        // Give the renderer content on the final blank line so it has a height.
        output.push('\u{200b}');
    }
    let log = scrollable(
        container(
            text(output)
                .width(Fill)
                .font(Font::MONOSPACE)
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
