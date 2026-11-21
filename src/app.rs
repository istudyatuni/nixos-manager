use std::{env, path::PathBuf, time::Duration};

use iced::{
    Center, Element, Fill, Subscription, Task, Theme, time,
    widget::{button, column, container, row, space, text},
};

use crate::{
    config::{self, Config, LoadError, SaveError},
    git,
    i18n::I18n,
    ui::{
        screens::{Screen, overview, settings},
        style::{self, buttons, containers, spacing},
    },
};

pub struct App {
    screen: Screen,
    i18n: I18n,
    theme: Theme,
    home: Option<PathBuf>,
    config: Option<Config>,
    loading: bool,
    load_error: bool,
    mode: overview::Mode,
    git_available: bool,
    has_local_changes: bool,
    download: overview::DownloadStatus,
    checking_repository: bool,
    repository_generation: u64,
}

#[derive(Clone)]
pub enum Message {
    OpenSettings,
    CloseSettings,
    Overview(overview::Message),
    Settings(settings::Message),
    Loaded(Result<Option<Config>, LoadError>, bool),
    Saved(Result<Config, SaveError>),
    Downloaded(Result<(), git::Error>),
    InspectRepository,
    RepositoryInspected(u64, bool),
}

impl App {
    pub fn boot() -> (Self, Task<Message>) {
        let app = Self {
            screen: Screen::Overview,
            i18n: I18n::system(),
            theme: style::theme(),
            home: env::home_dir(),
            config: None,
            loading: true,
            load_error: false,
            mode: overview::Mode::default(),
            git_available: false,
            has_local_changes: false,
            download: overview::DownloadStatus::Idle,
            checking_repository: false,
            repository_generation: 0,
        };
        let task = Task::perform(
            async {
                tokio::task::spawn_blocking(|| (config::load(), git::available()))
                    .await
                    .unwrap_or((Err(LoadError), false))
            },
            |(result, git_available)| Message::Loaded(result, git_available),
        );
        (app, task)
    }

    pub fn title(&self) -> String {
        self.i18n.text("app-title")
    }

    pub fn theme(&self) -> Theme {
        self.theme.clone()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        if self.can_inspect_repository() {
            time::every(Duration::from_secs(3)).map(|_| Message::InspectRepository)
        } else {
            Subscription::none()
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenSettings if !self.loading && matches!(self.screen, Screen::Overview) => {
                self.screen = Screen::Settings(settings::State::new(
                    self.config.as_ref(),
                    self.home.as_deref(),
                ));
            }
            Message::CloseSettings => {
                if matches!(&self.screen, Screen::Settings(state) if !state.saving()) {
                    self.screen = Screen::Overview;
                    return self.inspect_repository();
                }
            }
            Message::Overview(overview::Message::SelectMode(mode)) => {
                self.mode = mode;
            }
            Message::Overview(overview::Message::Download) => {
                if !self.git_available
                    || self.download.is_running()
                    || !matches!(self.screen, Screen::Overview)
                {
                    return Task::none();
                }
                if let Some(config) = self
                    .config
                    .as_ref()
                    .filter(|config| config.repository_path().is_some())
                    .cloned()
                {
                    self.download = overview::DownloadStatus::Running;
                    self.invalidate_repository_check();
                    return Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || git::download(&config))
                                .await
                                .unwrap_or_else(|error| Err(git::Error::Inspect(error.to_string())))
                        },
                        Message::Downloaded,
                    );
                }
            }
            Message::Downloaded(result) => {
                if result.is_ok() {
                    self.mode = overview::Mode::Test;
                    self.has_local_changes = false;
                } else if matches!(result, Err(git::Error::LocalChanges)) {
                    self.has_local_changes = true;
                }
                self.download = overview::DownloadStatus::Finished(result);
                return self.inspect_repository();
            }
            Message::InspectRepository => return self.inspect_repository(),
            Message::RepositoryInspected(generation, has_local_changes) => {
                if generation == self.repository_generation {
                    self.checking_repository = false;
                    self.has_local_changes = has_local_changes;
                }
            }
            Message::Loaded(result, git_available) => {
                self.loading = false;
                self.git_available = git_available;
                match result {
                    Ok(Some(config)) => {
                        self.i18n.set_language(config.language);
                        self.config = Some(config);
                    }
                    Ok(None) => {}
                    Err(LoadError) => self.load_error = true,
                }
                return self.inspect_repository();
            }
            Message::Settings(message) => {
                if let Screen::Settings(state) = &mut self.screen
                    && let Some(draft) = state.update(message, !self.download.is_running())
                {
                    let repository_required = self
                        .config
                        .as_ref()
                        .and_then(Config::repository_path)
                        .is_some();
                    return Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || {
                                config::save(draft, repository_required)
                            })
                            .await
                            .unwrap_or(Err(SaveError::Write))
                        },
                        Message::Saved,
                    );
                }
            }
            Message::Saved(result) => match result {
                Ok(config) => {
                    self.i18n.set_language(config.language);
                    self.config = Some(config);
                    self.load_error = false;
                    self.screen = Screen::Overview;
                    self.download = overview::DownloadStatus::Idle;
                    self.has_local_changes = false;
                    self.invalidate_repository_check();
                    return self.inspect_repository();
                }
                Err(error) => {
                    if let Screen::Settings(state) = &mut self.screen {
                        state.save_failed(error);
                    }
                }
            },
            Message::OpenSettings => {}
        }
        Task::none()
    }

    fn can_inspect_repository(&self) -> bool {
        matches!(self.screen, Screen::Overview)
            && self.git_available
            && !self.download.is_running()
            && self
                .config
                .as_ref()
                .and_then(Config::repository_path)
                .is_some()
    }

    fn inspect_repository(&mut self) -> Task<Message> {
        if !self.can_inspect_repository() || self.checking_repository {
            return Task::none();
        }
        let Some(config) = &self.config else {
            return Task::none();
        };
        let path = config.path.clone();
        let repository = config.repository.clone();
        let generation = self.repository_generation;
        self.checking_repository = true;
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    git::has_local_changes(&path, &repository).unwrap_or(false)
                })
                .await
                .unwrap_or(false)
            },
            move |dirty| Message::RepositoryInspected(generation, dirty),
        )
    }

    fn invalidate_repository_check(&mut self) {
        self.repository_generation += 1;
        self.checking_repository = false;
    }

    pub fn view(&self) -> Element<'_, Message> {
        let (title, action, navigation, content) = match &self.screen {
            Screen::Overview => {
                let repository =
                    self.config
                        .as_ref()
                        .and_then(Config::repository_path)
                        .map(|path| overview::Repository {
                            path: config::display_repo_path(path, self.home.as_deref()),
                            git_available: self.git_available,
                            has_local_changes: self.has_local_changes,
                            download: &self.download,
                        });
                (
                    self.title(),
                    self.i18n.text("app-open-settings"),
                    (!self.loading).then_some(Message::OpenSettings),
                    overview::view(
                        &self.i18n,
                        repository,
                        self.mode,
                        self.loading,
                        self.load_error,
                    )
                    .map(Message::Overview),
                )
            }
            Screen::Settings(state) => (
                self.i18n.text("app-open-settings"),
                self.i18n.text("app-back"),
                (!state.saving()).then_some(Message::CloseSettings),
                state
                    .view(&self.i18n, !self.download.is_running())
                    .map(Message::Settings),
            ),
        };
        let toolbar = row![
            text(title).size(22),
            space().width(Fill),
            button(text(action))
                .padding([7, 12])
                .style(buttons::secondary)
                .on_press_maybe(navigation),
        ]
        .align_y(Center)
        .spacing(spacing::LG);

        column![
            container(toolbar)
                .padding([10.0, spacing::PAGE_PADDING])
                .width(Fill)
                .style(containers::toolbar),
            content,
        ]
        .into()
    }
}
