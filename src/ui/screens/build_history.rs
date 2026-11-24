use fluent_bundle::FluentArgs;
use iced::{
    Center, Element, Fill, Font, Shrink, border, padding,
    widget::{
        button, column, container, row, rule, scrollable, space, text,
        text_editor::{self, Content},
        tooltip,
    },
};

use crate::{
    i18n::I18n,
    rebuild::{
        Error, ReadyTarget,
        history::{Build, Removal, Snapshot, Source, Target},
    },
    ui::{
        components::{icon::Icon, icon_button, popup, selectable_text},
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
    ToggleRemoval(Removal),
    Remove,
    SelectText(TextTarget, text_editor::Action),
}

#[derive(Clone, Copy)]
pub enum TextTarget {
    Path(usize),
    Generation(usize),
    Note(usize, usize),
    Confirmation,
}

pub enum Action {
    None,
    Close,
    Refresh,
    Activate(ReadyTarget),
    Remove(Vec<Removal>),
}

#[derive(Default)]
pub struct State {
    selection: Option<(ReadyTarget, Content)>,
    rows: Vec<TargetRow>,
    marked: Vec<Removal>,
    removing: bool,
    removal_error: Option<Error>,
}

impl State {
    pub fn update(
        &mut self,
        message: Message,
        snapshot: Option<&Snapshot>,
        can_activate: bool,
        can_remove_generations: bool,
    ) -> Action {
        match message {
            Message::Close if !self.removing => return Action::Close,
            Message::Refresh if !self.removing && snapshot.is_some() => return Action::Refresh,
            Message::Select(selection) if can_activate && !self.removing => {
                let path = Content::with_text(&selection.path().to_string_lossy());
                self.selection = Some((selection, path));
            }
            Message::Back => self.selection = None,
            Message::Activate if can_activate && !self.removing => {
                if let Some((selection, _)) = self.selection.take() {
                    return Action::Activate(selection);
                }
            }
            Message::ToggleRemoval(removal)
                if self.selection.is_none()
                    && !self.removing
                    && let Some(snapshot) = snapshot.filter(|snapshot| !snapshot.unavailable) =>
            {
                if let Some(index) = self.marked.iter().position(|marked| *marked == removal) {
                    self.marked.remove(index);
                } else if self.can_mark(snapshot, &removal) {
                    self.marked.push(removal);
                }
            }
            Message::Remove
                if self.selection.is_none()
                    && self.can_remove(snapshot, can_remove_generations) =>
            {
                self.removing = true;
                self.removal_error = None;
                return Action::Remove(self.marked.clone());
            }
            Message::SelectText(target, action) if !action.is_edit() => {
                let content = match target {
                    TextTarget::Path(row) => self.rows.get_mut(row).map(|row| &mut row.path),
                    TextTarget::Generation(row) => self
                        .rows
                        .get_mut(row)
                        .and_then(|row| row.generation.as_mut()),
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

    fn can_mark(&self, snapshot: &Snapshot, removal: &Removal) -> bool {
        self.rows
            .iter()
            .find(|row| row.removal.as_ref() == Some(removal))
            .is_some_and(|row| snapshot.targets[row.target_index].can_mark(removal, &self.marked))
    }

    fn system_marked(&self) -> bool {
        self.marked
            .iter()
            .any(|removal| removal.generation().is_some())
    }

    fn can_remove(&self, snapshot: Option<&Snapshot>, can_remove_generations: bool) -> bool {
        !self.removing
            && snapshot.is_some_and(|snapshot| !snapshot.unavailable)
            && !self.marked.is_empty()
            && (can_remove_generations || !self.system_marked())
    }

    pub fn removing(&self) -> bool {
        self.removing
    }

    pub fn removal_finished(&mut self, result: Result<(), Error>) {
        self.removing = false;
        self.removal_error = result.err();
        if result.is_ok() {
            self.marked.clear();
        }
    }

    pub fn set_snapshot(&mut self, i18n: &I18n, snapshot: &Snapshot) {
        self.rows.clear();
        for (index, target) in snapshot.targets.iter().enumerate() {
            let system = &target.system;
            for &number in &system.generations {
                self.rows
                    .push(TargetRow::new(i18n, index, target, Some(number)));
            }
            if system.generations.is_empty()
                || !target.builds.is_empty()
                || (system.profile_selected && system.selected_generation.is_none())
            {
                self.rows.push(TargetRow::new(i18n, index, target, None));
            }
        }
        if !snapshot.unavailable {
            for removal in std::mem::take(&mut self.marked) {
                if self.can_mark(snapshot, &removal) {
                    self.marked.push(removal);
                }
            }
        }
    }

    pub fn view<'a>(
        &'a self,
        i18n: &'a I18n,
        snapshot: Option<&'a Snapshot>,
        can_activate: bool,
        missing_tool: Option<&'static str>,
        missing_removal_tool: Option<&'static str>,
    ) -> Element<'a, Message> {
        modal::dialog(
            move || match &self.selection {
                Some((selection, path)) => confirmation(
                    i18n,
                    path,
                    can_activate,
                    snapshot.is_some_and(|snapshot| snapshot.warns_about_switch(selection.path())),
                ),
                None => list(
                    i18n,
                    snapshot,
                    self,
                    can_activate,
                    missing_tool,
                    missing_removal_tool,
                ),
            },
            (!self.removing).then_some(Message::Close),
        )
    }
}

fn list<'a>(
    i18n: &'a I18n,
    snapshot: Option<&'a Snapshot>,
    state: &'a State,
    can_activate: bool,
    missing_tool: Option<&str>,
    missing_removal_tool: Option<&str>,
) -> Element<'a, Message> {
    let activation_tool = missing_tool.filter(|_| {
        snapshot.is_some_and(|snapshot| snapshot.targets.iter().any(|target| !target.system.active))
    });
    let delete_label = if state.removing {
        "history-removing"
    } else {
        "history-remove-selected"
    };
    let actions = row![
        button(text(i18n.text(delete_label)))
            .padding([7, 12])
            .style(buttons::danger)
            .on_press_maybe(
                state
                    .can_remove(snapshot, missing_removal_tool.is_none())
                    .then_some(Message::Remove),
            ),
        space().width(Fill),
        tooltip(
            button(text(i18n.text("history-refresh")))
                .padding([7, 12])
                .style(buttons::secondary)
                .on_press_maybe(
                    (snapshot.is_some() && !state.removing).then_some(Message::Refresh)
                ),
            text(i18n.text("history-refresh-help"))
                .size(13)
                .wrapping(text::Wrapping::WordOrGlyph),
            tooltip::Position::Top,
        )
        .gap(spacing::SM)
        .style(containers::card),
    ]
    .spacing(spacing::MD);
    let mut deletion_notices = column![].spacing(spacing::MD);
    if let Some(error) = state.removal_error {
        let key = match error {
            Error::TargetUnavailable => "history-removal-changed",
            Error::Cancelled => "history-removal-cancelled",
            Error::AuthorizationFailed => "history-removal-auth-failed",
            _ => "history-removal-failed",
        };
        deletion_notices = deletion_notices.push(notice(i18n.text(key)).color(colors::DANGER));
    }
    if state.system_marked() {
        deletion_notices = deletion_notices.push(notice(i18n.text("history-removal-boot-warning")));
        if let Some(program) = missing_removal_tool
            && Some(program) != activation_tool
        {
            deletion_notices = deletion_notices.push(tool_notice(i18n, program));
        }
    }
    let header = row![
        text(i18n.text("history-title")).size(22),
        space().width(Fill),
        icon_button::view(
            text("×").size(20),
            i18n.text("history-close"),
            (!state.removing).then_some(Message::Close),
            buttons::secondary,
        ),
    ]
    .align_y(Center)
    .spacing(spacing::LG);
    let mut entries = column![].spacing(spacing::LG).width(Fill);
    if let Some(snapshot) = snapshot {
        if snapshot.unavailable {
            entries = entries.push(notice(i18n.text("history-unavailable")));
        }
        if let Some(program) = activation_tool {
            entries = entries.push(tool_notice(i18n, program));
        }
        if let Some(rollback) = &snapshot.quick_rollback {
            entries = entries.push(
                button(text(i18n.text("history-quick-rollback")))
                    .padding([7, 12])
                    .style(buttons::primary)
                    .on_press_maybe(
                        can_activate.then(|| Message::Select(ReadyTarget::Quick(rollback.clone()))),
                    ),
            );
        }
        if snapshot.targets.is_empty() && !snapshot.unavailable {
            entries = entries.push(text(i18n.text("history-empty")));
        }
        for (index, entry) in state.rows.iter().enumerate() {
            entries = entries.push(rule::horizontal(1)).push(target_row(
                i18n,
                &snapshot.targets[entry.target_index],
                entry,
                index,
                can_activate,
                &state.marked,
                !state.removing && !snapshot.unavailable,
            ));
        }
    } else {
        entries = entries.push(text(i18n.text("history-loading")));
    }
    let deletion_notices = (state.removal_error.is_some() || state.system_marked())
        .then(|| scrollable(deletion_notices).width(Fill).height(Shrink));
    // The body takes the height left over after laying out the header and actions.
    let body = column![scrollable(entries).height(Fill), deletion_notices].spacing(spacing::MD);
    column![header, body, actions].spacing(spacing::LG).into()
}

struct TargetRow {
    target_index: usize,
    path: Content,
    generation: Option<Content>,
    profile_selected: bool,
    notes: Vec<Content>,
    removal: Option<Removal>,
}

impl TargetRow {
    fn new(i18n: &I18n, target_index: usize, target: &Target, number: Option<u64>) -> Self {
        let notes = if number.is_none() {
            target
                .builds
                .iter()
                .map(|build| Content::with_text(&build_label(i18n, build)))
                .collect()
        } else {
            Vec::new()
        };
        Self {
            target_index,
            path: Content::with_text(&target.store_path.to_string_lossy()),
            generation: number.map(|number| Content::with_text(&generation_label(i18n, number))),
            profile_selected: target.system.profile_selected
                && number == target.system.selected_generation,
            notes,
            removal: target.removal(number),
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
    labels: &'a TargetRow,
    index: usize,
    can_activate: bool,
    marked: &[Removal],
    can_toggle: bool,
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
        (target.system.active, "history-active", colors::SUCCESS),
        (target.system.booted, "history-booted", colors::INFO),
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
    if labels.profile_selected {
        details = details.push(profile_caption(i18n));
    }
    if let Some(generation) = &labels.generation {
        let color = if labels.profile_selected {
            colors::INFO
        } else {
            colors::TEXT_MUTED
        };
        details = details.push(selectable_text::view(
            generation,
            Font::DEFAULT,
            13,
            color,
            move |action| Message::SelectText(TextTarget::Generation(index), action),
        ));
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
        labels.removal.as_ref().map(|removal| {
            let selected = marked.contains(removal);
            let (label, style) = if selected {
                ("history-unmark-removal", buttons::danger as _)
            } else {
                ("history-mark-removal", buttons::secondary as _)
            };
            icon_button::view(
                Icon::Trash,
                i18n.text(label),
                (can_toggle && (selected || target.can_mark(removal, marked)))
                    .then(|| Message::ToggleRemoval(removal.clone())),
                style,
            )
        }),
        (!target.system.active).then(|| {
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

fn tool_notice<'a>(i18n: &I18n, program: &str) -> iced::widget::Text<'a> {
    let mut args = FluentArgs::new();
    args.set("program", program);
    notice(i18n.with_args("overview-rebuild-tool-unavailable", &args))
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
        scrollable(
            column![
                selectable_text::view(path, Font::DEFAULT, 16, colors::TEXT, |action| {
                    Message::SelectText(TextTarget::Confirmation, action)
                }),
                notice(i18n.text(warning)),
            ]
            .spacing(spacing::LG),
        )
        .height(Fill),
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

fn notice<'a>(message: String) -> iced::widget::Text<'a> {
    text(message)
        .size(14)
        .color(colors::WARNING)
        .wrapping(text::Wrapping::WordOrGlyph)
}
