use fluent_bundle::FluentArgs;
use iced::{
    Center, Color, Element, Fill, Font, Right, Shrink, padding,
    widget::{
        button, column, container, mouse_area, row, rule, scrollable, space, stack, text,
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
        components::{icon, icon_button, popup, selectable_text},
        modal,
        style::{buttons, colors, containers, spacing},
    },
};

#[derive(Clone)]
pub enum Message {
    Close,
    Dismiss,
    Refresh,
    HoverRollback(bool),
    Select {
        target: ReadyTarget,
        generation: Option<u64>,
    },
    Back,
    Activate,
    ToggleRemoval(Removal),
    ClearMarked,
    Remove,
    SelectText(TextTarget, text_editor::Action),
}

#[derive(Clone, Copy)]
pub enum TextTarget {
    Row(usize, DetailText),
    Confirmation(DetailText),
}

#[derive(Clone, Copy)]
pub enum DetailText {
    Path,
    Generation(usize),
    Note(usize),
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
    selection: Option<(ReadyTarget, TargetDetails)>,
    rollback_hovered: bool,
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
            Message::Dismiss if self.can_dismiss() => return Action::Close,
            Message::Refresh if !self.removing && snapshot.is_some() => {
                self.rollback_hovered = false;
                return Action::Refresh;
            }
            Message::HoverRollback(hovered) => self.rollback_hovered = hovered,
            Message::Select { target, generation } if can_activate && !self.removing => {
                let mut rows = self.rows.iter().filter(|row| {
                    snapshot
                        .and_then(|snapshot| snapshot.targets.get(row.target_index))
                        .is_some_and(|entry| entry.store_path == target.path())
                });
                let details = match &target {
                    ReadyTarget::Selected(_) => rows
                        .find(|row| row.generation == generation)
                        .map(|row| row.details.clone()),
                    ReadyTarget::Quick(_) => rows.next().map(|first| {
                        let mut details = first.details.clone();
                        for row in rows {
                            details
                                .generations
                                .extend(row.details.generations.iter().cloned());
                            details.notes.extend(row.details.notes.iter().cloned());
                            details.profile_without_generation |=
                                row.details.profile_without_generation;
                        }
                        details
                    }),
                }
                .unwrap_or_else(|| TargetDetails {
                    path: Content::with_text(&target.path().to_string_lossy()),
                    ..TargetDetails::default()
                });
                self.rollback_hovered = false;
                self.selection = Some((target, details));
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
            Message::ClearMarked if self.can_clear_marked() => self.marked.clear(),
            Message::Remove
                if self.selection.is_none()
                    && self.can_remove(snapshot, can_remove_generations) =>
            {
                self.removing = true;
                self.removal_error = None;
                return Action::Remove(self.marked.clone());
            }
            Message::SelectText(target, action) if !action.is_edit() => {
                let (details, field) = match target {
                    TextTarget::Row(row, field) => {
                        (self.rows.get_mut(row).map(|row| &mut row.details), field)
                    }
                    TextTarget::Confirmation(field) => {
                        (self.selection.as_mut().map(|(_, details)| details), field)
                    }
                };
                if let Some(content) = details.and_then(|details| details.text_mut(field)) {
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

    fn can_dismiss(&self) -> bool {
        !self.removing && self.marked.is_empty()
    }

    fn can_clear_marked(&self) -> bool {
        self.selection.is_none() && !self.removing && !self.marked.is_empty()
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
                Some((selection, details)) => confirmation(
                    i18n,
                    details,
                    can_activate,
                    snapshot.is_some_and(|snapshot| snapshot.warns_about_switch(selection.path())),
                    !self.marked.is_empty(),
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
            self.can_dismiss().then_some(Message::Dismiss),
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
    let rollback = snapshot.and_then(|snapshot| snapshot.quick_rollback.as_ref());
    let rollback_hint = match (snapshot, rollback, missing_tool) {
        (None, _, _) => i18n.text("history-loading"),
        (Some(snapshot), _, _) if snapshot.unavailable => i18n.text("history-unavailable"),
        (_, None, _) => i18n.text("history-quick-rollback-no-target"),
        _ if state.removing => i18n.text("history-removing"),
        (_, _, Some(program)) => missing_tool_message(i18n, program),
        _ => i18n.text("history-quick-rollback-help"),
    };
    let rollback_button = button(text(i18n.text("history-quick-rollback")))
        .padding([7, 12])
        .style(buttons::primary)
        .on_press_maybe(
            rollback
                .filter(|_| can_activate)
                .map(|rollback| Message::Select {
                    target: ReadyTarget::Quick(rollback.clone()),
                    generation: None,
                }),
        );
    let rollback_action = tooltip(
        mouse_area(rollback_button)
            .on_enter(Message::HoverRollback(true))
            .on_exit(Message::HoverRollback(false)),
        text(rollback_hint)
            .size(13)
            .wrapping(text::Wrapping::WordOrGlyph),
        tooltip::Position::Top,
    )
    .gap(spacing::SM)
    .style(containers::card);
    let right_actions = row![
        icon_button::view(
            icon::refresh(),
            i18n.text("history-refresh-help"),
            (snapshot.is_some() && !state.removing).then_some(Message::Refresh),
            buttons::secondary,
        ),
        rollback_action,
    ]
    .align_y(Center)
    .spacing(spacing::MD);
    let delete = button(text(i18n.text(delete_label)).wrapping(text::Wrapping::WordOrGlyph))
        .padding([7, 12])
        .style(buttons::danger)
        .on_press_maybe(
            state
                .can_remove(snapshot, missing_removal_tool.is_none())
                .then_some(Message::Remove),
        );
    let clear_marked = icon_button::view(
        icon::clear_selection(),
        i18n.text("history-clear-selection"),
        state.can_clear_marked().then_some(Message::ClearMarked),
        buttons::secondary,
    );
    // Reserve the icon's full width before laying out the wrapping delete label.
    let removal_actions = stack![
        container(delete).padding(padding::right(icon_button::SIZE + spacing::MD)),
        container(clear_marked)
            .align_x(Right)
            .align_y(Center)
            .width(Fill)
            .height(Fill),
    ];
    let actions = row![container(removal_actions).width(Fill), right_actions]
        .align_y(Center)
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
            deletion_notices = deletion_notices.push(notice(missing_tool_message(i18n, program)));
        }
    }
    let header = row![
        text(i18n.text("history-title")).size(22),
        space().width(Fill),
        icon_button::close(
            i18n.text("app-close"),
            (!state.removing).then_some(Message::Close),
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
            entries = entries.push(notice(missing_tool_message(i18n, program)));
        }
        if snapshot.targets.is_empty() && !snapshot.unavailable {
            entries = entries.push(text(i18n.text("history-empty")));
        }
        for index in 0..state.rows.len() {
            entries = entries.push(rule::horizontal(1)).push(target_row(
                i18n,
                snapshot,
                state,
                index,
                can_activate,
            ));
        }
    } else {
        entries = entries.push(text(i18n.text("history-loading")));
    }
    let deletion_notices = (state.removal_error.is_some() || state.system_marked())
        .then(|| scrollable(deletion_notices).width(Fill).height(Shrink));
    // The body takes the height left over after laying out the header and actions.
    let body = column![
        scrollable(entries).height(Fill).spacing(spacing::MD),
        deletion_notices,
    ]
    .spacing(spacing::MD);
    column![header, body, actions].spacing(spacing::LG).into()
}

struct TargetRow {
    target_index: usize,
    generation: Option<u64>,
    details: TargetDetails,
    removal: Option<Removal>,
}

impl TargetRow {
    fn new(i18n: &I18n, target_index: usize, target: &Target, number: Option<u64>) -> Self {
        Self {
            target_index,
            generation: number,
            details: TargetDetails::new(i18n, target, number),
            removal: target.removal(number),
        }
    }
}

#[derive(Clone, Default)]
struct TargetDetails {
    path: Content,
    generations: Vec<GenerationDetails>,
    booted: bool,
    profile_without_generation: bool,
    notes: Vec<Content>,
}

#[derive(Clone)]
struct GenerationDetails {
    label: Content,
    profile_selected: bool,
}

impl TargetDetails {
    fn new(i18n: &I18n, target: &Target, number: Option<u64>) -> Self {
        let notes = if number.is_none() {
            target
                .builds
                .iter()
                .map(|build| Content::with_text(&build_label(i18n, build)))
                .collect()
        } else {
            Vec::new()
        };
        let profile_selected =
            target.system.profile_selected && number == target.system.selected_generation;
        Self {
            path: Content::with_text(&target.store_path.to_string_lossy()),
            generations: number
                .into_iter()
                .map(|number| GenerationDetails {
                    label: Content::with_text(&generation_label(i18n, number)),
                    profile_selected,
                })
                .collect(),
            booted: target.system.booted,
            profile_without_generation: number.is_none() && profile_selected,
            notes,
        }
    }

    fn text_mut(&mut self, field: DetailText) -> Option<&mut Content> {
        match field {
            DetailText::Path => Some(&mut self.path),
            DetailText::Generation(index) => self
                .generations
                .get_mut(index)
                .map(|entry| &mut entry.label),
            DetailText::Note(line) => self.notes.get_mut(line),
        }
    }

    fn view<'a>(
        &'a self,
        i18n: &I18n,
        active: bool,
        text_target: impl Fn(DetailText) -> TextTarget + Copy + 'a,
    ) -> Element<'a, Message> {
        let mut details = column![selectable_text::view(
            &self.path,
            Font::DEFAULT,
            16,
            colors::TEXT,
            move |action| Message::SelectText(text_target(DetailText::Path), action),
        )]
        .spacing(spacing::SM)
        .width(Fill);
        if active || self.booted {
            let statuses = row![
                active.then(|| caption_with_help(
                    i18n.text("history-active"),
                    i18n.text("history-active-help"),
                    colors::SUCCESS,
                )),
                self.booted.then(|| caption_with_help(
                    i18n.text("history-booted"),
                    i18n.text("history-booted-help"),
                    colors::PURPLE,
                )),
            ]
            .align_y(Center)
            .spacing(spacing::LG)
            .wrap();
            details = details.push(statuses);
        }
        if self.profile_without_generation {
            details = details.push(profile_caption(i18n));
        }
        for (index, generation) in self.generations.iter().enumerate() {
            let color = if generation.profile_selected {
                details = details.push(profile_caption(i18n));
                colors::INFO
            } else {
                colors::TEXT_MUTED
            };
            details = details.push(selectable_text::view(
                &generation.label,
                Font::DEFAULT,
                13,
                color,
                move |action| {
                    Message::SelectText(text_target(DetailText::Generation(index)), action)
                },
            ));
        }
        for (line, note) in self.notes.iter().enumerate() {
            details = details.push(selectable_text::view(
                note,
                Font::DEFAULT,
                13,
                colors::TEXT_MUTED,
                move |action| Message::SelectText(text_target(DetailText::Note(line)), action),
            ));
        }
        details.into()
    }
}

fn generation_label(i18n: &I18n, number: u64) -> String {
    let mut args = FluentArgs::new();
    args.set("number", number.to_string());
    i18n.with_args("history-generation", &args)
}

fn target_row<'a>(
    i18n: &I18n,
    snapshot: &Snapshot,
    state: &'a State,
    index: usize,
    can_activate: bool,
) -> Element<'a, Message> {
    let labels = &state.rows[index];
    let target = &snapshot.targets[labels.target_index];
    let marked = &state.marked;
    let can_toggle = !state.removing && !snapshot.unavailable;
    let highlight_activation = state.rollback_hovered
        && snapshot
            .quick_rollback
            .as_ref()
            .is_some_and(|rollback| rollback.target == target.store_path);
    let details = labels
        .details
        .view(i18n, target.system.active, move |field| {
            TextTarget::Row(index, field)
        });
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
                icon::trash(),
                i18n.text(label),
                (can_toggle && (selected || target.can_mark(removal, marked)))
                    .then(|| Message::ToggleRemoval(removal.clone())),
                style,
            )
        }),
        (!target.system.active).then(|| {
            icon_button::view(
                // Shift the triangle right to balance its wider left edge.
                container(icon::play()).padding(padding::left(2)),
                i18n.text("history-activate"),
                can_activate.then(|| Message::Select {
                    target: ReadyTarget::Selected(target.store_path.clone()),
                    generation: labels.generation,
                }),
                if highlight_activation {
                    buttons::highlighted
                } else {
                    buttons::secondary
                },
            )
        }),
    ]
    .spacing(spacing::MD)
    .align_y(Center)
    .into()
}

fn missing_tool_message(i18n: &I18n, program: &str) -> String {
    let mut args = FluentArgs::new();
    args.set("program", program);
    i18n.with_args("overview-rebuild-tool-unavailable", &args)
}

fn profile_caption(i18n: &I18n) -> Element<'static, Message> {
    caption_with_help(
        i18n.text("history-system-profile"),
        i18n.text("history-system-profile-help"),
        colors::INFO,
    )
}

fn caption_with_help(label: String, help: String, color: Color) -> Element<'static, Message> {
    row![text(label).size(13).color(color), popup::help(help)]
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
    details: &'a TargetDetails,
    can_activate: bool,
    warns_about_switch: bool,
    show_close: bool,
) -> Element<'a, Message> {
    let warning = if warns_about_switch {
        "history-switch-warning"
    } else {
        "history-activation-warning"
    };
    column![
        row![
            text(i18n.text("history-activate-confirm")).size(22),
            show_close.then(|| space().width(Fill)),
            show_close.then(|| icon_button::close(i18n.text("app-close"), Some(Message::Close))),
        ]
        .align_y(Center)
        .spacing(spacing::LG),
        scrollable(
            column![
                details.view(i18n, false, TextTarget::Confirmation),
                (!details.booted).then(|| notice(i18n.text(warning))),
            ]
            .spacing(spacing::LG),
        )
        .height(Fill)
        .spacing(spacing::MD),
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
