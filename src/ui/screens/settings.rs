use std::{
    fmt,
    path::{Path, PathBuf},
};

use fluent_bundle::FluentArgs;
use iced::{
    Center, Element, Fill,
    task::Handle,
    widget::{
        Column, Row, TextInput, button, column, container, pick_list, row, scrollable, space, text,
        text_input,
    },
};

use crate::{
    config::{
        CONFIG_DIRECTORY, Config, DEFAULT_FLAKE_DIRECTORY, DEFAULT_REPO_PATH, SaveError, UI_SCALES,
        display_repo_path, github_repository,
    },
    i18n::{I18n, LanguagePreference},
    ui::style::{buttons, colors, inputs, pick_lists, spacing},
};

mod repository_input;
use repository_input::RepositoryInput;

#[derive(Clone)]
pub enum Message {
    LanguageChanged(LanguagePreference),
    ScaleChanged(u16),
    RepositoryChanged(String),
    TokenChanged(String),
    PathChanged(String),
    BranchChanged(String),
    FlakeDirectoryChanged(String),
    InputFocused,
    Save,
}

pub enum Action {
    None,
    LookupBranches,
    Save(Config),
}

pub struct BranchLookup {
    pub repository: String,
    pub path: Option<PathBuf>,
    pub token: String,
    pub check_head: bool,
}

enum Lookup<T> {
    Pending,
    Running { id: u64, _handle: Handle },
    Finished(T),
}

impl<T> Lookup<T> {
    fn finish(&mut self, id: u64, result: T) -> bool {
        if !matches!(self, Self::Running { id: current, .. } if *current == id) {
            return false;
        }
        *self = Self::Finished(result);
        true
    }
}

pub struct State {
    original: Box<Config>,
    draft: Config,
    error: Option<SaveError>,
    saving: bool,
    git_available: bool,
    repository: Option<String>,
    branches: Lookup<Vec<String>>,
    head: Lookup<Option<String>>,
    auto_branch: bool,
    save_after_head: bool,
}

impl State {
    pub fn new(config: Option<&Config>, home: Option<&Path>, git_available: bool) -> Self {
        let mut draft = config.cloned().unwrap_or_default();
        draft.path = display_repo_path(&draft.path, home).into_owned();
        Self {
            original: Box::new(draft.clone()),
            repository: github_repository(draft.repository.trim()),
            draft,
            error: None,
            saving: false,
            git_available,
            branches: Lookup::Pending,
            head: Lookup::Pending,
            auto_branch: false,
            save_after_head: false,
        }
    }

    pub fn saving(&self) -> bool {
        self.saving
    }

    pub fn ui_scale(&self) -> u16 {
        self.draft.ui_scale
    }

    pub fn save_failed(&mut self, error: SaveError) {
        self.saving = false;
        self.error = Some(error);
    }

    fn can_save(&self, operation_idle: bool) -> bool {
        operation_idle && !self.saving && self.draft != *self.original
    }

    fn needs_head(&self) -> bool {
        self.git_available
            && self
                .repository
                .as_deref()
                .is_some_and(|repository| !same_repository(repository, &self.original.repository))
    }

    pub fn branch_lookup(&self, allow_head: bool) -> Option<BranchLookup> {
        let repository = self.repository.as_ref()?;
        let local_pending = matches!(self.branches, Lookup::Pending);
        let check_head = allow_head && self.needs_head() && matches!(self.head, Lookup::Pending);
        if !self.git_available || (!local_pending && !check_head) {
            return None;
        }
        Some(BranchLookup {
            repository: repository.clone(),
            path: local_pending.then(|| self.draft.path.clone()),
            token: self.draft.token.trim().to_owned(),
            check_head,
        })
    }

    pub fn branches_started(&mut self, id: u64, handle: Handle) {
        self.branches = Lookup::Running {
            id,
            _handle: handle.abort_on_drop(),
        };
    }

    pub fn head_started(&mut self, id: u64, handle: Handle) {
        self.head = Lookup::Running {
            id,
            _handle: handle.abort_on_drop(),
        };
    }

    pub fn branches_finished(&mut self, id: u64, branches: Vec<String>) {
        self.branches.finish(id, branches);
    }

    pub fn refresh_local_branches(&mut self) {
        self.branches = Lookup::Pending;
    }

    pub fn head_finished(
        &mut self,
        id: u64,
        branch: Option<String>,
        operation_idle: bool,
    ) -> Option<Config> {
        if !self.head.finish(id, branch) {
            return None;
        }
        if self.draft.branch.trim().is_empty()
            && let Lookup::Finished(Some(branch)) = &self.head
        {
            self.draft.branch.clone_from(branch);
            self.auto_branch = true;
        }
        (std::mem::take(&mut self.save_after_head) && self.can_save(operation_idle))
            .then(|| self.start_save())
    }

    fn start_save(&mut self) -> Config {
        self.saving = true;
        self.draft.clone()
    }

    pub fn update(&mut self, message: Message, operation_idle: bool) -> Action {
        if self.saving {
            return Action::None;
        }
        match message {
            Message::InputFocused => return Action::LookupBranches,
            Message::Save => {
                if !self.can_save(operation_idle) || self.save_after_head {
                    return Action::None;
                }
                self.error = None;
                if self.needs_head() && !matches!(self.head, Lookup::Finished(_)) {
                    self.save_after_head = true;
                    return Action::LookupBranches;
                }
                return Action::Save(self.start_save());
            }
            _ => {}
        }

        self.error = None;
        self.save_after_head = false;
        match message {
            Message::LanguageChanged(value) => self.draft.language = value,
            Message::ScaleChanged(value) => self.draft.ui_scale = value,
            Message::RepositoryChanged(value) => {
                let repository = github_repository(value.trim());
                if !same_repository(
                    self.repository.as_deref().unwrap_or_default(),
                    repository.as_deref().unwrap_or_default(),
                ) {
                    self.refresh_local_branches();
                    self.head = Lookup::Pending;
                    if std::mem::take(&mut self.auto_branch) {
                        self.draft.branch.clear();
                    }
                }
                self.repository = repository;
                self.draft.repository = value;
            }
            Message::TokenChanged(value) => {
                if value.trim() != self.draft.token.trim()
                    && !matches!(self.head, Lookup::Finished(Some(_)))
                {
                    self.head = Lookup::Pending;
                }
                self.draft.token = value;
            }
            Message::PathChanged(value) => {
                self.draft.path = value.into();
                self.refresh_local_branches();
            }
            Message::BranchChanged(value) => {
                self.draft.branch = value;
                self.auto_branch = false;
            }
            Message::FlakeDirectoryChanged(value) => self.draft.flake_directory = value.into(),
            Message::InputFocused | Message::Save => unreachable!(),
        }
        Action::None
    }

    pub fn view<'a>(&'a self, i18n: &'a I18n, operation_idle: bool) -> Element<'a, Message> {
        let editing = !self.saving;
        let submit =
            (self.can_save(operation_idle) && !self.save_after_head).then_some(Message::Save);
        let repository = text_input(
            &i18n.text("settings-repository-placeholder"),
            &self.draft.repository,
        )
        .on_input_maybe(editing.then_some(Message::RepositoryChanged))
        .on_submit_maybe(submit.clone());
        let token = text_input("", &self.draft.token)
            .secure(true)
            .on_input_maybe(editing.then_some(Message::TokenChanged))
            .on_submit_maybe(submit.clone());
        let mut path_args = FluentArgs::new();
        path_args.set("path", DEFAULT_REPO_PATH);
        let path = text_input(
            &i18n.with_args("settings-path-placeholder", &path_args),
            &self.draft.path.to_string_lossy(),
        )
        .on_input_maybe(editing.then_some(Message::PathChanged))
        .on_submit_maybe(submit.clone());
        let branch_editing = editing
            && self.repository.is_some()
            && (!self.needs_head() || matches!(self.head, Lookup::Finished(_)));
        let branch = text_input(
            &i18n.text("settings-branch-placeholder"),
            &self.draft.branch,
        )
        .on_input_maybe(branch_editing.then_some(Message::BranchChanged))
        .on_submit_maybe(submit.clone());
        let branches = match &self.branches {
            Lookup::Finished(branches) if branch_editing => branches.as_slice(),
            _ => &[],
        };
        let mut branch = field(
            i18n.text("settings-branch"),
            RepositoryInput::new(styled_input(branch), editing)
                .branches(&self.draft.branch, branches)
                .into_element(),
        );
        let branch_status = match (&self.head, &self.branches) {
            (Lookup::Running { .. }, _) => Some(("settings-branch-loading-head", colors::INFO)),
            (Lookup::Finished(None), _) => {
                Some(("settings-branch-head-unavailable", colors::TEXT_MUTED))
            }
            (_, Lookup::Running { .. }) => Some(("settings-branch-loading-local", colors::INFO)),
            _ => None,
        };
        if let Some((key, color)) = branch_status {
            branch = branch.push(text(i18n.text(key)).size(13).color(color));
        }
        let flake_directory = text_input(
            DEFAULT_FLAKE_DIRECTORY,
            &self.draft.flake_directory.to_string_lossy(),
        )
        .on_input_maybe(editing.then_some(Message::FlakeDirectoryChanged))
        .on_submit_maybe(submit.clone());

        let interface = column![
            text(i18n.text("settings-interface-section")).size(18),
            language_field(i18n, self.draft.language, editing),
            scale_field(i18n, self.draft.ui_scale, editing),
        ]
        .spacing(spacing::LG);
        let repository = column![
            text(i18n.text("settings-repository-section")).size(18),
            field(i18n.text("settings-repository"), styled_input(repository)),
            field(
                i18n.text("settings-token"),
                RepositoryInput::new(styled_input(token), editing).into_element()
            ),
            field(
                i18n.text("settings-path"),
                RepositoryInput::new(styled_input(path), editing).into_element()
            )
            .push(
                text(i18n.text("settings-path-hint"))
                    .size(13)
                    .color(colors::TEXT_MUTED)
            ),
            branch,
            field(
                i18n.text("settings-flake-directory"),
                RepositoryInput::new(styled_input(flake_directory), editing).into_element(),
            )
            .push(
                text(i18n.text("settings-flake-directory-hint"))
                    .size(13)
                    .color(colors::TEXT_MUTED)
            ),
        ]
        .spacing(spacing::XL);

        let mut form = column![interface, repository].spacing(spacing::XL);

        if let Some(error) = self.error {
            form = form.push(text(error_text(i18n, error)).size(14).color(colors::DANGER));
        }
        if !operation_idle {
            form = form.push(
                text(i18n.text("settings-operation-in-progress"))
                    .size(14)
                    .color(colors::INFO),
            );
        }

        let save_label = if self.saving {
            "settings-saving"
        } else {
            "settings-save"
        };
        let save = button(text(i18n.text(save_label)))
            .padding([7, 12])
            .style(buttons::primary)
            .on_press_maybe(submit);
        form = form.push(container(save).align_right(Fill));

        let form = container(form).width(Fill).max_width(488);
        scrollable(
            container(form)
                .padding(spacing::PAGE_PADDING)
                .center_x(Fill),
        )
        .height(Fill)
        .into()
    }
}

#[derive(Clone, PartialEq)]
struct SelectOption<T> {
    value: T,
    label: String,
}

impl<T> fmt::Display for SelectOption<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

fn language_field(i18n: &I18n, selected: LanguagePreference, editing: bool) -> Row<'_, Message> {
    let options = [
        (LanguagePreference::System, "settings-language-system"),
        (LanguagePreference::English, "settings-language-english"),
        (LanguagePreference::Russian, "settings-language-russian"),
    ]
    .into_iter()
    .map(|(value, key)| SelectOption {
        value,
        label: i18n.text(key),
    })
    .collect();
    select_field(
        i18n.text("settings-language"),
        options,
        selected,
        editing,
        Message::LanguageChanged,
    )
}

fn scale_field(i18n: &I18n, selected: u16, editing: bool) -> Row<'_, Message> {
    let mut args = FluentArgs::new();
    let options = UI_SCALES
        .into_iter()
        .map(|value| {
            args.set("percent", value);
            SelectOption {
                value,
                label: i18n.with_args("settings-scale-percent", &args),
            }
        })
        .collect();
    select_field(
        i18n.text("settings-scale"),
        options,
        selected,
        editing,
        Message::ScaleChanged,
    )
}

fn select_field<'a, T: Clone + PartialEq + 'a>(
    label: String,
    options: Vec<SelectOption<T>>,
    selected: T,
    editing: bool,
    on_select: impl Fn(T) -> Message + 'a,
) -> Row<'a, Message> {
    let selected = options
        .iter()
        .find(|option| option.value == selected)
        .cloned();

    let width = 176;
    let control: Element<'_, Message> = if editing {
        pick_list(options, selected, move |option| on_select(option.value))
            .width(width)
            .text_size(15)
            .padding([8, 10])
            .style(pick_lists::field)
            .menu_style(pick_lists::menu)
            .into()
    } else {
        text_input("", selected.as_ref().map_or("", |option| &option.label))
            .width(width)
            .size(15)
            .padding([8, 10])
            .style(inputs::input)
            .into()
    };

    row![text(label).size(14), space().width(Fill), control,]
        .align_y(Center)
        .spacing(spacing::LG)
}

fn same_repository(first: &str, second: &str) -> bool {
    let first = first.strip_suffix(".git").unwrap_or(first);
    let second = second.strip_suffix(".git").unwrap_or(second);
    first.eq_ignore_ascii_case(second)
}

fn styled_input(input: TextInput<'_, Message>) -> TextInput<'_, Message> {
    input.size(15).padding([8, 10]).style(inputs::input)
}

fn field<'a>(label: String, input: impl Into<Element<'a, Message>>) -> Column<'a, Message> {
    column![text(label).size(14), input.into()].spacing(spacing::SM)
}

fn error_text(i18n: &I18n, error: SaveError) -> String {
    match error {
        SaveError::Repository => i18n.text("settings-error-repository"),
        SaveError::Token => i18n.text("settings-error-token"),
        SaveError::Path => i18n.text("settings-error-path"),
        SaveError::Branch => i18n.text("settings-error-branch"),
        SaveError::FlakeDirectory => i18n.text("settings-error-flake-directory"),
        SaveError::Write => {
            let mut args = FluentArgs::new();
            args.set("directory", format!("~/{CONFIG_DIRECTORY}"));
            i18n.with_args("settings-error-write", &args)
        }
    }
}
