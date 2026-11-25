use std::{
    fmt,
    path::{Path, PathBuf},
};

use fluent_bundle::FluentArgs;
use iced::{
    Center, Element, Fill,
    task::Handle,
    widget::{Column, Row, button, column, pick_list, row, scrollable, space, text},
};

use crate::{
    config::{
        CONFIG_DIRECTORY, Config, DEFAULT_REPO_PATH, RepositoryUrlError, SaveError, UI_SCALES,
        display_repo_path, github_repository, valid_token,
    },
    git::{self, Branch, RemoteBranchesError, RemoteRefs},
    i18n::{I18n, LanguagePreference},
    ui::{
        components::{icon, icon_button, text_input::text_input},
        modal,
        style::{buttons, colors, pick_lists, spacing},
    },
};

mod repository_input;
use repository_input::RepositoryInput;

#[derive(Clone)]
pub enum Message {
    LanguageChanged(LanguagePreference),
    ScaleChanged(u16),
    RepositoryChanged(String),
    RepositoryBlurred,
    TokenChanged(String),
    PathChanged(String),
    BranchChanged(String),
    FlakeDirectoryChanged(String),
    InputFocused,
    LoadRemoteBranches,
    Cancel,
    BackdropPressed,
    Save,
}

pub enum Action {
    None,
    Close,
    LookupBranches(Option<RemoteQuery>),
    PreviewLanguage(LanguagePreference),
    Save(Config),
}

pub struct BranchLookup {
    pub repository: String,
    pub path: PathBuf,
    pub token: String,
    pub check_local: bool,
    pub remote: Option<RemoteQuery>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemoteQuery {
    Head,
    Branches,
}

enum Lookup<T, Query = ()> {
    Pending,
    Running {
        id: u64,
        query: Query,
        _handle: Handle,
    },
    Finished(T),
}

impl<T, Query> Lookup<T, Query> {
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
    repository: Result<String, RepositoryUrlError>,
    repository_error: Option<RepositoryUrlError>,
    local: Lookup<Vec<Branch>>,
    remote: Lookup<Result<RemoteRefs, RemoteBranchesError>, RemoteQuery>,
    branch_names: Vec<String>,
    auto_branch: bool,
    save_after_lookup: bool,
}

impl State {
    pub fn new(config: Option<&Config>, home: Option<&Path>, git_available: bool) -> Self {
        let mut draft = config.cloned().unwrap_or_default();
        draft.path = display_repo_path(&draft.path, home).into_owned();
        Self {
            original: Box::new(draft.clone()),
            repository: github_repository(draft.repository.trim()),
            repository_error: None,
            draft,
            error: None,
            saving: false,
            git_available,
            local: Lookup::Pending,
            remote: Lookup::Pending,
            branch_names: Vec::new(),
            auto_branch: false,
            save_after_lookup: false,
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

    fn has_changes(&self) -> bool {
        self.draft != *self.original
    }

    fn can_save(&self, operation_idle: bool) -> bool {
        operation_idle && !self.saving && self.has_changes()
    }

    fn can_dismiss_from_backdrop(&self) -> bool {
        !self.saving && !self.has_changes()
    }

    fn repository_validation_error(&self) -> Option<RepositoryUrlError> {
        if self.original.repository_path().is_some() || self.draft.requires_repository() {
            self.repository.as_ref().err().copied()
        } else {
            None
        }
    }

    fn needs_head(&self) -> bool {
        self.git_available
            && self
                .repository
                .as_deref()
                .is_ok_and(|repository| !same_repository(repository, &self.original.repository))
    }

    fn needs_lookup_to_save(&self) -> bool {
        self.git_available
            && self.repository.is_ok()
            && (self.needs_head() || self.draft.token.trim() != self.original.token.trim())
            && match self.remote {
                Lookup::Finished(Ok(_)) => false,
                Lookup::Finished(Err(RemoteBranchesError::Network)) => {
                    self.draft.token.trim().is_empty()
                }
                _ => true,
            }
    }

    fn can_query_remote(&self, query: RemoteQuery) -> bool {
        self.git_available
            && self.repository.is_ok()
            && valid_token(self.draft.token.trim())
            && match &self.remote {
                Lookup::Pending => true,
                Lookup::Running { query: running, .. } => {
                    query == RemoteQuery::Branches && *running == RemoteQuery::Head
                }
                Lookup::Finished(Ok(refs)) => {
                    query == RemoteQuery::Branches && refs.branches.is_none()
                }
                Lookup::Finished(Err(_)) => query == RemoteQuery::Branches,
            }
    }

    pub fn branch_lookup(&self, remote: Option<RemoteQuery>) -> Option<BranchLookup> {
        let repository = self.repository.as_ref().ok()?;
        let check_local = matches!(self.local, Lookup::Pending);
        let remote = remote.filter(|query| self.can_query_remote(*query));
        if !self.git_available || (!check_local && remote.is_none()) {
            return None;
        }
        Some(BranchLookup {
            repository: repository.clone(),
            path: self.draft.path.clone(),
            token: self.draft.token.trim().to_owned(),
            check_local,
            remote,
        })
    }

    pub fn local_branches_started(&mut self, id: u64, handle: Handle) {
        self.local = Lookup::Running {
            id,
            query: (),
            _handle: handle.abort_on_drop(),
        };
    }

    pub fn remote_lookup_started(&mut self, id: u64, query: RemoteQuery, handle: Handle) {
        self.remote = Lookup::Running {
            id,
            query,
            _handle: handle.abort_on_drop(),
        };
    }

    pub fn local_branches_finished(&mut self, id: u64, branches: Vec<Branch>) {
        if self.local.finish(id, branches) {
            self.update_branch_names();
        }
    }

    pub fn refresh_local_branches(&mut self) {
        if !matches!(self.local, Lookup::Pending) {
            self.local = Lookup::Pending;
            self.update_branch_names();
        }
    }

    pub fn remote_lookup_finished(
        &mut self,
        id: u64,
        result: Result<RemoteRefs, RemoteBranchesError>,
        operation_idle: bool,
    ) -> Option<Config> {
        if !self.remote.finish(id, result) {
            return None;
        }
        self.update_branch_names();
        if self.needs_head()
            && self.draft.branch.trim().is_empty()
            && let Lookup::Finished(Ok(remote)) = &self.remote
            && let Some(branch) = &remote.default_branch
        {
            self.draft.branch.clone_from(branch);
            self.auto_branch = true;
        }
        (std::mem::take(&mut self.save_after_lookup)
            && self.can_save(operation_idle)
            && !self.needs_lookup_to_save())
        .then(|| self.start_save())
    }

    fn update_branch_names(&mut self) {
        let local = match &self.local {
            Lookup::Finished(branches) => branches.as_slice(),
            _ => &[],
        };
        let remote = match &self.remote {
            Lookup::Finished(Ok(remote)) => remote.branches.as_deref().unwrap_or_default(),
            _ => &[],
        };
        self.branch_names = git::branch_names(local, remote);
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
            Message::Cancel => return Action::Close,
            Message::BackdropPressed => {
                return if self.can_dismiss_from_backdrop() {
                    Action::Close
                } else {
                    Action::None
                };
            }
            Message::InputFocused => return Action::LookupBranches(None),
            Message::RepositoryBlurred => {
                self.repository_error = self.repository_validation_error();
                return Action::None;
            }
            Message::LoadRemoteBranches => {
                return Action::LookupBranches(Some(RemoteQuery::Branches));
            }
            Message::Save => {
                if !self.can_save(operation_idle) || self.save_after_lookup {
                    return Action::None;
                }
                self.error = None;
                if let Some(error) = self.repository_validation_error() {
                    self.repository_error = Some(error);
                    return Action::None;
                }
                if !valid_token(self.draft.token.trim()) {
                    self.error = Some(SaveError::Token);
                    return Action::None;
                }
                if self.needs_lookup_to_save() {
                    if matches!(self.remote, Lookup::Finished(Err(_))) {
                        self.remote = Lookup::Pending;
                    }
                    self.save_after_lookup = true;
                    return Action::LookupBranches(Some(RemoteQuery::Head));
                }
                return Action::Save(self.start_save());
            }
            _ => {}
        }

        self.error = None;
        self.save_after_lookup = false;
        match message {
            Message::LanguageChanged(value) => {
                self.draft.language = value;
                return Action::PreviewLanguage(value);
            }
            Message::ScaleChanged(value) => self.draft.ui_scale = value,
            Message::RepositoryChanged(value) => {
                let repository = github_repository(value.trim());
                if !same_repository(
                    self.repository.as_deref().unwrap_or_default(),
                    repository.as_deref().unwrap_or_default(),
                ) {
                    self.local = Lookup::Pending;
                    self.remote = Lookup::Pending;
                    self.branch_names.clear();
                    if std::mem::take(&mut self.auto_branch) {
                        self.draft.branch.clear();
                    }
                }
                self.repository = repository;
                self.repository_error = None;
                self.draft.repository = value;
            }
            Message::TokenChanged(value) => {
                if value.trim() != self.draft.token.trim()
                    && !matches!(self.remote, Lookup::Pending)
                {
                    self.remote = Lookup::Pending;
                    self.update_branch_names();
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
            Message::RepositoryBlurred
            | Message::InputFocused
            | Message::LoadRemoteBranches
            | Message::Cancel
            | Message::BackdropPressed
            | Message::Save => unreachable!(),
        }
        Action::None
    }

    pub fn view<'a>(&'a self, i18n: &'a I18n, operation_idle: bool) -> Element<'a, Message> {
        modal::dialog(
            move || self.panel(i18n, operation_idle),
            self.can_dismiss_from_backdrop()
                .then_some(Message::BackdropPressed),
        )
    }

    fn panel<'a>(&'a self, i18n: &'a I18n, operation_idle: bool) -> Element<'a, Message> {
        let editing = !self.saving;
        let submit =
            (self.can_save(operation_idle) && !self.save_after_lookup).then_some(Message::Save);
        let (repository_error, footer_error) = match self.error {
            Some(SaveError::Repository(error)) => (Some(error), None),
            error => (self.repository_error, error),
        };
        let repository = text_input(
            &i18n.text("settings-repository-placeholder"),
            &self.draft.repository,
        )
        .invalid(repository_error.is_some())
        .on_input_maybe(editing.then_some(Message::RepositoryChanged))
        .on_submit_maybe(submit.clone());
        let mut repository = field(
            i18n.text("settings-repository"),
            RepositoryInput::new(repository, false)
                .on_blur(Message::RepositoryBlurred)
                .into_element(),
        );
        if let Some(error) = repository_error {
            repository = repository.push(
                text(error_text(i18n, SaveError::Repository(error)))
                    .size(13)
                    .color(colors::DANGER),
            );
        }
        let token = text_input("", &self.draft.token)
            .secure(true)
            .on_input_maybe(editing.then_some(Message::TokenChanged))
            .on_submit_maybe(submit.clone());
        let mut token = field(
            i18n.text("settings-token"),
            RepositoryInput::new(token, editing).into_element(),
        )
        .push(
            text(i18n.text("settings-token-hint"))
                .size(13)
                .color(colors::TEXT_MUTED),
        );
        if matches!(
            self.remote,
            Lookup::Finished(Err(RemoteBranchesError::Unavailable))
        ) {
            token = token.push(
                text(i18n.text("settings-repository-unavailable"))
                    .size(13)
                    .color(colors::DANGER),
            );
        }
        let mut path_args = FluentArgs::new();
        path_args.set("path", DEFAULT_REPO_PATH);
        let path = text_input(
            &i18n.with_args("settings-path-placeholder", &path_args),
            &self.draft.path.to_string_lossy(),
        )
        .on_input_maybe(editing.then_some(Message::PathChanged))
        .on_submit_maybe(submit.clone());
        let branch_editing = editing && self.repository.is_ok();
        let branch = text_input(
            &i18n.text("settings-branch-placeholder"),
            &self.draft.branch,
        )
        .on_input_maybe(branch_editing.then_some(Message::BranchChanged))
        .on_submit_maybe(submit.clone());
        let mut branch_input = RepositoryInput::new(branch, branch_editing);
        if branch_editing && self.git_available {
            branch_input =
                branch_input.branches(&self.draft.branch, &self.branch_names, self.auto_branch);
        }
        let load_branches_tooltip = match &self.remote {
            Lookup::Running {
                query: RemoteQuery::Branches,
                ..
            } => "settings-branch-loading-remote-tooltip",
            Lookup::Finished(Ok(refs)) if refs.branches.is_some() => {
                "settings-branch-loaded-remote-tooltip"
            }
            _ => "settings-branch-load-remote",
        };
        let load_branches = icon_button::view(
            icon::cloud_download(),
            i18n.text(load_branches_tooltip),
            (editing && self.can_query_remote(RemoteQuery::Branches))
                .then_some(Message::LoadRemoteBranches),
            buttons::secondary,
        );
        let mut branch = field(
            i18n.text("settings-branch"),
            row![branch_input.into_element(), load_branches]
                .align_y(Center)
                .spacing(spacing::SM),
        );
        let branch_status = match &self.remote {
            Lookup::Running {
                query: RemoteQuery::Head,
                ..
            } => Some(("settings-branch-checking-remote", colors::INFO)),
            Lookup::Running { .. } => Some(("settings-branch-loading-remote", colors::INFO)),
            Lookup::Finished(Err(RemoteBranchesError::Network)) => {
                Some(("settings-branch-network-warning", colors::WARNING))
            }
            Lookup::Finished(Ok(remote))
                if self.needs_head()
                    && self.draft.branch.trim().is_empty()
                    && remote.default_branch.is_none() =>
            {
                Some(("settings-branch-head-unavailable", colors::TEXT_MUTED))
            }
            _ if matches!(self.local, Lookup::Running { .. }) => {
                Some(("settings-branch-loading-local", colors::INFO))
            }
            _ => None,
        };
        if let Some((key, color)) = branch_status {
            branch = branch.push(text(i18n.text(key)).size(13).color(color));
        }
        let flake_directory = text_input(
            &i18n.text("settings-flake-directory-placeholder"),
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
            repository,
            token,
            field(
                i18n.text("settings-path"),
                RepositoryInput::new(path, editing).into_element()
            )
            .push(
                text(i18n.text("settings-path-hint"))
                    .size(13)
                    .color(colors::TEXT_MUTED)
            ),
            branch,
            field(
                i18n.text("settings-flake-directory"),
                RepositoryInput::new(flake_directory, editing).into_element(),
            )
            .push(
                text(i18n.text("settings-flake-directory-hint"))
                    .size(13)
                    .color(colors::TEXT_MUTED)
            ),
        ]
        .spacing(spacing::XL);

        let form = column![interface, repository].spacing(spacing::XL);

        let mut footer = Column::new().spacing(spacing::MD);
        if let Some(error) = footer_error {
            footer = footer.push(text(error_text(i18n, error)).size(14).color(colors::DANGER));
        }
        if !operation_idle {
            footer = footer.push(
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
        let cancel = button(text(i18n.text("settings-cancel")))
            .padding([7, 12])
            .style(buttons::secondary)
            .on_press_maybe(editing.then_some(Message::Cancel));
        let header = row![
            text(i18n.text("app-open-settings")).size(22),
            space().width(Fill),
            icon_button::close(i18n.text("app-close"), editing.then_some(Message::Cancel)),
        ]
        .align_y(Center)
        .spacing(spacing::LG);
        let footer = footer.push(row![cancel, space().width(Fill), save].spacing(spacing::LG));
        column![
            header,
            scrollable(form)
                .width(Fill)
                .height(Fill)
                .spacing(spacing::MD),
            footer,
        ]
        .spacing(spacing::XL)
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
            .handle(icon::pick_list_handle())
            .width(width)
            .text_size(15)
            .padding([8, 10])
            .style(pick_lists::field)
            .menu_style(pick_lists::menu)
            .into()
    } else {
        text_input("", selected.as_ref().map_or("", |option| &option.label))
            .width(width)
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

fn field<'a>(label: String, input: impl Into<Element<'a, Message>>) -> Column<'a, Message> {
    column![text(label).size(14), input.into()].spacing(spacing::SM)
}

fn error_text(i18n: &I18n, error: SaveError) -> String {
    match error {
        SaveError::Repository(error) => i18n.text(match error {
            RepositoryUrlError::InvalidUrl => "settings-repository-invalid-hint",
            RepositoryUrlError::RequiresHttps => "settings-repository-https-hint",
            RepositoryUrlError::InvalidDomain => "settings-repository-domain-hint",
            RepositoryUrlError::InvalidPath => "settings-repository-path-hint",
        }),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saving_a_new_repository_only_requests_head() {
        let mut state = State::new(None, None, true);
        state.update(
            Message::RepositoryChanged("https://github.com/example/repo".into()),
            true,
        );
        assert!(matches!(
            state.update(Message::Save, true),
            Action::LookupBranches(Some(RemoteQuery::Head))
        ));
        let (_, handle) = iced::Task::<()>::none().abortable();
        state.remote_lookup_started(1, RemoteQuery::Head, handle);
        assert!(matches!(
            state
                .branch_lookup(Some(RemoteQuery::Branches))
                .unwrap()
                .remote,
            Some(RemoteQuery::Branches)
        ));
        let draft = state
            .remote_lookup_finished(
                1,
                Ok(RemoteRefs {
                    branches: None,
                    default_branch: Some("main".to_owned()),
                }),
                true,
            )
            .unwrap();
        assert_eq!(draft.branch, "main");
        let lookup = state.branch_lookup(Some(RemoteQuery::Branches)).unwrap();
        assert!(matches!(lookup.remote, Some(RemoteQuery::Branches)));
    }
}
