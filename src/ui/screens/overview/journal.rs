use fluent_bundle::FluentArgs;
use iced::{
    Element, Fill, Font,
    widget::{column, container, scrollable, text},
};

use super::Message;
use crate::{
    i18n::I18n,
    rebuild::{Mode, Outcome, Run},
    ui::style::{colors, containers, spacing},
};

pub fn view<'a>(i18n: &I18n, run: &Run) -> Element<'a, Message> {
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
    .height(Fill)
    .anchor_bottom();

    column![
        text(i18n.with_args(key, &args)).size(14).color(color),
        container(log)
            .width(Fill)
            .height(Fill)
            .style(containers::journal),
    ]
    .width(Fill)
    .height(Fill)
    .spacing(spacing::SM)
    .into()
}
