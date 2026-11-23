use fluent_bundle::FluentArgs;
use iced::{
    Center, Element, Fill, Font, Shrink, border, padding,
    widget::{
        button, column, container, row, rule, scrollable, space, text,
        text_editor::{self, Content},
    },
};

use crate::{
    i18n::I18n,
    rebuild::{
        ReadyTarget,
        history::{Build, Snapshot, Source, Target},
    },
    ui::{
        components::{icon_button, popup, selectable_text},
        modal,
        style::{buttons, colors, containers, spacing},
    },
};

#[derive(Clone)]
pub enum Message {
    Close,
    Refresh,
    Select(ReadyTarget),
    Back,
    Activate,
    SelectText(TextTarget, text_editor::Action),
}

#[derive(Clone, Copy)]
pub enum TextTarget {
    Path(usize),
    ProfileGeneration(usize),
    Note(usize, usize),
    Confirmation,
}

pub enum Action {
    None,
    Close,
    Refresh,
    Activate(ReadyTarget),
}

#[derive(Default)]
pub struct State {
    selection: Option<(ReadyTarget, Content)>,
    rows: Vec<TargetText>,
}

impl State {
    pub fn update(&mut self, message: Message, can_activate: bool) -> Action {
        match message {
            Message::Close => return Action::Close,
            Message::Refresh => return Action::Refresh,
            Message::Select(selection) if can_activate => {
                let path = Content::with_text(&selection.path().to_string_lossy());
                self.selection = Some((selection, path));
            }
            Message::Back => self.selection = None,
            Message::Activate if can_activate => {
                if let Some((selection, _)) = self.selection.take() {
                    return Action::Activate(selection);
                }
            }
            Message::SelectText(target, action) if !action.is_edit() => {
                let content = match target {
                    TextTarget::Path(row) => self.rows.get_mut(row).map(|row| &mut row.path),
                    TextTarget::ProfileGeneration(row) => self
                        .rows
                        .get_mut(row)
                        .and_then(|row| row.profile_generation.as_mut()),
                    TextTarget::Note(row, note) => self
                        .rows
                        .get_mut(row)
                        .and_then(|row| row.notes.get_mut(note)),
                    TextTarget::Confirmation => self.selection.as_mut().map(|(_, path)| path),
                };
                if let Some(content) = content {
                    content.perform(action);
                }
            }
            _ => {}
        }
        Action::None
    }

    pub fn set_targets(&mut self, i18n: &I18n, targets: &[Target]) {
        self.rows = targets
            .iter()
            .map(|target| TargetText::new(i18n, target))
            .collect();
    }

    pub fn view<'a>(
        &'a self,
        i18n: &'a I18n,
        snapshot: Option<&'a Snapshot>,
        can_activate: bool,
        missing_tool: Option<&'static str>,
    ) -> Element<'a, Message> {
        modal::dialog(
            move || match &self.selection {
                Some((selection, path)) => confirmation(
                    i18n,
                    path,
                    can_activate,
                    snapshot.is_some_and(|snapshot| snapshot.warns_about_switch(selection.path())),
                ),
                None => list(i18n, snapshot, &self.rows, can_activate, missing_tool),
            },
            Some(Message::Close),
        )
    }
}

fn list<'a>(
    i18n: &'a I18n,
    snapshot: Option<&'a Snapshot>,
    rows: &'a [TargetText],
    can_activate: bool,
    missing_tool: Option<&str>,
) -> Element<'a, Message> {
    let actions = row![
        space().width(Fill),
        button(text(i18n.text("history-refresh")))
            .padding([7, 12])
            .style(buttons::secondary)
            .on_press_maybe(snapshot.is_some().then_some(Message::Refresh)),
        button(text(i18n.text("history-close")))
            .padding([7, 12])
            .style(buttons::secondary)
            .on_press(Message::Close),
    ]
    .spacing(spacing::MD);
    let mut content = column![text(i18n.text("history-title")).size(22)].spacing(spacing::LG);
    let Some(snapshot) = snapshot else {
        return content
            .push(text(i18n.text("history-loading")))
            .push(actions)
            .into();
    };
    if snapshot.unavailable {
        content = content.push(notice(i18n.text("history-unavailable")));
    }
    if let Some(program) = missing_tool
        && snapshot.targets.iter().any(|target| !target.active)
    {
        let mut args = FluentArgs::new();
        args.set("program", program);
        content = content.push(notice(
            i18n.with_args("overview-rebuild-tool-unavailable", &args),
        ));
    }
    if let Some(rollback) = &snapshot.quick_rollback {
        content = content.push(
            button(text(i18n.text("history-quick-rollback")))
                .padding([7, 12])
                .style(buttons::primary)
                .on_press_maybe(
                    can_activate.then(|| Message::Select(ReadyTarget::Quick(rollback.clone()))),
                ),
        );
    }
    if snapshot.targets.is_empty() {
        if !snapshot.unavailable {
            content = content.push(text(i18n.text("history-empty")));
        }
        return content.push(actions).into();
    }
    let mut entries = column![].spacing(spacing::LG).width(Fill);
    for (index, (target, labels)) in snapshot.targets.iter().zip(rows).enumerate() {
        entries = entries.push(rule::horizontal(1)).push(target_row(
            i18n,
            target,
            labels,
            index,
            can_activate,
        ));
    }
    content
        .push(scrollable(entries).height(Shrink))
        .push(actions)
        .into()
}

struct TargetText {
    path: Content,
    profile_generation: Option<Content>,
    notes: Vec<Content>,
}

impl TargetText {
    fn new(i18n: &I18n, target: &Target) -> Self {
        let mut notes = Vec::new();
        let other_generations = target
            .generations
            .iter()
            .copied()
            .filter(|number| Some(*number) != target.selected_generation)
            .map(|number| generation_label(i18n, number))
            .collect::<Vec<_>>();
        if !other_generations.is_empty() {
            notes.push(Content::with_text(&other_generations.join(" · ")));
        }
        notes.extend(
            target
                .builds
                .iter()
                .map(|build| Content::with_text(&build_label(i18n, build))),
        );
        Self {
            path: Content::with_text(&target.store_path.to_string_lossy()),
            profile_generation: target
                .selected_generation
                .map(|number| Content::with_text(&generation_label(i18n, number))),
            notes,
        }
    }
}

fn generation_label(i18n: &I18n, number: u64) -> String {
    let mut args = FluentArgs::new();
    args.set("number", number.to_string());
    i18n.with_args("history-generation", &args)
}

fn target_row<'a>(
    i18n: &I18n,
    target: &Target,
    labels: &'a TargetText,
    index: usize,
    can_activate: bool,
) -> Element<'a, Message> {
    let mut details = column![selectable_text::view(
        &labels.path,
        Font::DEFAULT,
        16,
        colors::TEXT,
        move |action| { Message::SelectText(TextTarget::Path(index), action) }
    )]
    .spacing(spacing::SM)
    .width(Fill);
    let badges = [
        (target.active, "history-active", colors::SUCCESS),
        (target.booted, "history-booted", colors::INFO),
    ];
    let statuses = row(badges
        .into_iter()
        .filter(|(show, _, _)| *show)
        .map(|(_, key, color)| text(i18n.text(key)).size(13).color(color).into()))
    .spacing(spacing::LG)
    .wrap();
    if badges.iter().any(|(show, _, _)| *show) {
        details = details.push(statuses);
    }
    if target.profile_selected {
        let generation = labels.profile_generation.as_ref().map(|content| {
            selectable_text::view(content, Font::DEFAULT, 13, colors::INFO, move |action| {
                Message::SelectText(TextTarget::ProfileGeneration(index), action)
            })
        });
        details = details.push(column![profile_caption(i18n), generation].spacing(spacing::SM));
    }
    for (line, note) in labels.notes.iter().enumerate() {
        details = details.push(selectable_text::view(
            note,
            Font::DEFAULT,
            13,
            colors::TEXT_MUTED,
            move |action| Message::SelectText(TextTarget::Note(index, line), action),
        ));
    }
    row![
        details,
        (!target.active).then(|| {
            icon_button::view(
                // Shift the triangle right to balance its wider left edge.
                container(text("▶").size(14)).padding(padding::left(2)),
                i18n.text("history-activate"),
                can_activate
                    .then(|| Message::Select(ReadyTarget::Selected(target.store_path.clone()))),
                buttons::secondary,
            )
        }),
    ]
    .spacing(spacing::MD)
    .align_y(Center)
    .into()
}

fn profile_caption(i18n: &I18n) -> Element<'static, Message> {
    const ICON_SIZE: f32 = 16.0;
    let icon = container(text("?").size(11).color(colors::INFO))
        .center(ICON_SIZE)
        .style(|_| {
            container::Style::default().border(
                border::rounded(ICON_SIZE / 2.0)
                    .width(1)
                    .color(colors::INFO),
            )
        });
    let help = popup::on_hover(
        icon,
        container(
            text(i18n.text("history-system-profile-help"))
                .size(13)
                .width(240)
                .wrapping(text::Wrapping::WordOrGlyph),
        )
        .padding(spacing::MD)
        .style(containers::card),
    );
    row![
        text(i18n.text("history-system-profile"))
            .size(13)
            .color(colors::INFO),
        help,
    ]
    .align_y(Center)
    .spacing(spacing::SM)
    .into()
}

fn build_label(i18n: &I18n, build: &Build) -> String {
    let mut args = FluentArgs::new();
    let unknown = i18n.text("history-unknown");
    let key = match &build.source {
        Source::Imported { discovered_at } => {
            args.set("timestamp", discovered_at.timestamp());
            "history-imported"
        }
        Source::Built { branch, revision } => {
            args.set("branch", branch.as_deref().unwrap_or(&unknown));
            args.set(
                "revision",
                revision
                    .as_deref()
                    .map(|revision| revision.get(..8).unwrap_or(revision))
                    .unwrap_or(&unknown),
            );
            if let Some(time) = build.built_at {
                args.set("timestamp", time.timestamp());
                "history-built"
            } else {
                "history-built-without-date"
            }
        }
    };
    i18n.with_args(key, &args)
}

fn confirmation<'a>(
    i18n: &I18n,
    path: &'a Content,
    can_activate: bool,
    warns_about_switch: bool,
) -> Element<'a, Message> {
    let warning = if warns_about_switch {
        "history-switch-warning"
    } else {
        "history-activation-warning"
    };
    column![
        text(i18n.text("history-activate-confirm")).size(22),
        selectable_text::view(path, Font::DEFAULT, 16, colors::TEXT, |action| {
            Message::SelectText(TextTarget::Confirmation, action)
        }),
        notice(i18n.text(warning)),
        row![
            space().width(Fill),
            button(text(i18n.text("history-back")))
                .padding([7, 12])
                .style(buttons::secondary)
                .on_press(Message::Back),
            button(text(i18n.text("history-activate")))
                .padding([7, 12])
                .style(buttons::primary)
                .on_press_maybe(can_activate.then_some(Message::Activate)),
        ]
        .spacing(spacing::MD)
    ]
    .spacing(spacing::LG)
    .into()
}

fn notice(message: String) -> iced::widget::Text<'static> {
    text(message)
        .size(14)
        .color(colors::WARNING)
        .wrapping(text::Wrapping::WordOrGlyph)
}
