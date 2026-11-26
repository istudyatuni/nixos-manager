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
        style::{TOOLTIP_TEXT_SIZE, buttons, colors, containers, spacing},
    },
};

#[derive(Clone)]
pub enum Message {
    Close,
    Dismiss,
    Refresh,
    HoverRollback(bool),
    Select(ReadyTarget),
    Activate,
    ToggleRemoval(Removal),
    ClearMarked,
    Remove,
    SelectText(TextTarget, text_editor::Action),
}

#[derive(Clone, Copy)]
pub enum TextTarget {
    Group(usize, DetailText),
    Confirmation(DetailText),
}

#[derive(Clone, Copy)]
pub enum DetailText {
    Path,
    Entry(usize),
}

pub enum Action {
    None,
    Close,
    Refresh,
    Activate(ReadyTarget),
    Remove(Vec<Removal>),
    CopyPrimary(String),
}

#[derive(Default)]
pub struct State {
    selection: Option<(ReadyTarget, TargetDetails)>,
    rollback_hovered: bool,
    groups: Vec<TargetDetails>,
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
            Message::Dismiss if self.can_dismiss() => {
                if self.selection.take().is_none() {
                    return Action::Close;
                }
            }
            Message::Refresh if !self.removing && snapshot.is_some() => {
                self.rollback_hovered = false;
                return Action::Refresh;
            }
            Message::HoverRollback(hovered) => self.rollback_hovered = hovered,
            Message::Select(target) if can_activate && !self.removing => {
                let details = snapshot
                    .and_then(|snapshot| {
                        snapshot
                            .targets
                            .iter()
                            .position(|entry| entry.store_path == target.path())
                    })
                    .and_then(|index| self.groups.get(index))
                    .cloned()
                    .unwrap_or_else(|| TargetDetails {
                        path: Content::with_text(&target.path().to_string_lossy()),
                        ..TargetDetails::default()
                    });
                self.rollback_hovered = false;
                self.selection = Some((target, details));
            }
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
            Message::SelectText(target, action) => {
                let (details, field) = match target {
                    TextTarget::Group(index, field) => (self.groups.get_mut(index), field),
                    TextTarget::Confirmation(field) => {
                        (self.selection.as_mut().map(|(_, details)| details), field)
                    }
                };
                if let Some(content) = details.and_then(|details| details.text_mut(field))
                    && let Some(text) = selectable_text::perform(content, action)
                {
                    return Action::CopyPrimary(text);
                }
            }
            _ => {}
        }
        Action::None
    }

    fn can_mark(&self, snapshot: &Snapshot, removal: &Removal) -> bool {
        self.groups
            .iter()
            .zip(&snapshot.targets)
            .find(|(group, _)| {
                group
                    .entries
                    .iter()
                    .any(|entry| entry.removal.as_ref() == Some(removal))
            })
            .is_some_and(|(_, target)| target.can_mark(removal, &self.marked))
    }

    fn can_dismiss(&self) -> bool {
        !self.removing && (self.selection.is_some() || self.marked.is_empty())
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
        self.groups = snapshot
            .targets
            .iter()
            .map(|target| TargetDetails::new(i18n, target))
            .collect();
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
    let rollback_tooltip = match (snapshot, rollback, missing_tool) {
        (None, _, _) => i18n.text("history-quick-rollback-loading-tooltip"),
        (Some(snapshot), _, _) if snapshot.unavailable => {
            i18n.text("history-quick-rollback-unavailable-tooltip")
        }
        (_, None, _) => i18n.text("history-quick-rollback-no-target-tooltip"),
        _ if state.removing => i18n.text("history-quick-rollback-removing-tooltip"),
        (_, _, Some(program)) => {
            let mut args = FluentArgs::new();
            args.set("program", program);
            i18n.with_args("history-quick-rollback-tool-unavailable-tooltip", &args)
        }
        _ => i18n.text("history-quick-rollback-tooltip"),
    };
    let rollback_button = button(text(i18n.text("history-quick-rollback")))
        .padding([7, 12])
        .style(buttons::primary)
        .on_press_maybe(
            rollback
                .filter(|_| can_activate)
                .map(|rollback| Message::Select(ReadyTarget::Quick(rollback.clone()))),
        );
    let rollback_action = tooltip(
        mouse_area(rollback_button)
            .on_enter(Message::HoverRollback(true))
            .on_exit(Message::HoverRollback(false)),
        text(rollback_tooltip)
            .size(TOOLTIP_TEXT_SIZE)
            .wrapping(text::Wrapping::WordOrGlyph),
        tooltip::Position::Top,
    )
    .gap(spacing::SM)
    .style(containers::card);
    let right_actions = row![
        icon_button::view(
            icon::refresh(),
            i18n.text("history-refresh-tooltip"),
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
        for index in 0..state.groups.len() {
            entries = entries.push(rule::horizontal(1)).push(target_group(
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

#[derive(Clone, Default)]
struct TargetDetails {
    path: Content,
    entries: Vec<EntryDetails>,
    booted: bool,
    profile_without_generation: bool,
}

#[derive(Clone)]
struct EntryDetails {
    label: Content,
    profile_selected: bool,
    removal: Option<Removal>,
}

impl TargetDetails {
    fn new(i18n: &I18n, target: &Target) -> Self {
        let system = &target.system;
        let generations = system.generations.iter().map(|&number| EntryDetails {
            label: Content::with_text(&generation_label(i18n, number)),
            profile_selected: system.profile_selected && system.selected_generation == Some(number),
            removal: target.generation_removal(number),
        });
        let builds = target.builds.iter().map(|build| EntryDetails {
            label: Content::with_text(&build_label(i18n, build)),
            profile_selected: false,
            removal: target.build_removal(build),
        });
        Self {
            path: Content::with_text(&target.store_path.to_string_lossy()),
            entries: generations.chain(builds).collect(),
            booted: system.booted,
            profile_without_generation: system.profile_selected
                && system.selected_generation.is_none(),
        }
    }

    fn text_mut(&mut self, field: DetailText) -> Option<&mut Content> {
        match field {
            DetailText::Path => Some(&mut self.path),
            DetailText::Entry(index) => self.entries.get_mut(index).map(|entry| &mut entry.label),
        }
    }

    fn view<'a>(
        &'a self,
        i18n: &I18n,
        active: bool,
        text_target: impl Fn(DetailText) -> TextTarget + Copy + 'a,
        activation: Option<Element<'a, Message>>,
        removal_button: impl Fn(&Removal) -> Option<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        let path =
            selectable_text::view(&self.path, Font::DEFAULT, 16, colors::TEXT, move |action| {
                Message::SelectText(text_target(DetailText::Path), action)
            });
        let heading = row![path, activation].align_y(Center).spacing(spacing::MD);
        let mut details = column![heading].spacing(spacing::SM).width(Fill);
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
        let entry_padding = padding::horizontal(spacing::LG);
        for (index, entry) in self.entries.iter().enumerate() {
            if index > 0 {
                let divider = rule::horizontal(1).style(|theme| rule::Style {
                    color: colors::BORDER_MUTED,
                    ..rule::default(theme)
                });
                details = details.push(
                    container(divider)
                        .padding(entry_padding.right(spacing::LG + icon_button::SIZE)),
                );
            }
            let color = if entry.profile_selected {
                details = details.push(profile_caption(i18n));
                colors::INFO
            } else {
                colors::TEXT_MUTED
            };
            let label =
                selectable_text::view(&entry.label, Font::DEFAULT, 13, color, move |action| {
                    Message::SelectText(text_target(DetailText::Entry(index)), action)
                });
            let removal = entry
                .removal
                .as_ref()
                .and_then(&removal_button)
                .map(|button| container(button).padding(padding::right(icon_button::SIZE)));
            let entry = row![label, removal].align_y(Center).spacing(spacing::MD);
            details = details.push(container(entry).padding(entry_padding));
        }
        details.into()
    }
}

fn generation_label(i18n: &I18n, number: u64) -> String {
    let mut args = FluentArgs::new();
    args.set("number", number.to_string());
    i18n.with_args("history-generation", &args)
}

fn target_group<'a>(
    i18n: &I18n,
    snapshot: &Snapshot,
    state: &'a State,
    index: usize,
    can_activate: bool,
) -> Element<'a, Message> {
    let target = &snapshot.targets[index];
    let marked = &state.marked;
    let can_toggle = !state.removing && !snapshot.unavailable;
    let highlight_activation = state.rollback_hovered
        && snapshot
            .quick_rollback
            .as_ref()
            .is_some_and(|rollback| rollback.target == target.store_path);
    let activation = (!target.system.active).then(|| {
        icon_button::view(
            icon::play(),
            i18n.text("history-activate"),
            can_activate.then(|| Message::Select(ReadyTarget::Selected(target.store_path.clone()))),
            if highlight_activation {
                buttons::highlighted
            } else {
                buttons::secondary
            },
        )
    });
    state.groups[index].view(
        i18n,
        target.system.active,
        move |field| TextTarget::Group(index, field),
        activation,
        |removal| {
            let selected = marked.contains(removal);
            let (label, style) = if selected {
                ("history-unmark-removal", buttons::danger as _)
            } else {
                ("history-mark-removal", buttons::secondary as _)
            };
            Some(icon_button::view(
                icon::trash(),
                i18n.text(label),
                (can_toggle && (selected || target.can_mark(removal, marked)))
                    .then(|| Message::ToggleRemoval(removal.clone())),
                style,
            ))
        },
    )
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
        "history-switch-non-system-profile-warning"
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
                details.view(i18n, false, TextTarget::Confirmation, None, |_| None),
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
                .on_press(Message::Dismiss),
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
