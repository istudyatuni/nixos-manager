use std::{env, path::PathBuf, time::Duration};

use iced::{
    Center, Element, Fill, Subscription, Task, Theme,
    futures::SinkExt,
    stream, time,
    widget::{button, column, container, row, space, text},
    window,
};
use tokio::sync::mpsc;

use crate::{
    config::{self, Config, LoadError, SaveError},
    git,
    i18n::I18n,
    rebuild::{self, Mode},
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
    mode: Mode,
    git_available: bool,
    rebuild_tools: rebuild::Tools,
    working_copy: git::WorkingCopy,
    download: overview::DownloadStatus,
    rebuild: Option<rebuild::Run>,
    rebuild_interrupt: Option<mpsc::Sender<()>>,
    pending_close: Option<window::Id>,
    overview: overview::State,
    checking_repository: bool,
    repository_generation: u64,
}

#[derive(Clone)]
pub enum Message {
    WindowCloseRequested(window::Id),
    OpenSettings,
    CloseSettings,
    Overview(overview::Message),
    Settings(settings::Message),
    Loaded(Result<Option<Config>, LoadError>, bool, rebuild::Tools),
    Saved(Result<Config, SaveError>),
    Downloaded(Result<(), git::Error>),
    Rebuild(rebuild::Event),
    InspectRepository,
    RepositoryInspected(u64, git::WorkingCopy),
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
            mode: Mode::default(),
            git_available: false,
            rebuild_tools: rebuild::Tools::default(),
            working_copy: git::WorkingCopy::default(),
            download: overview::DownloadStatus::Idle,
            rebuild: None,
            rebuild_interrupt: None,
            pending_close: None,
            overview: overview::State::default(),
            checking_repository: false,
            repository_generation: 0,
        };
        let task = Task::perform(
            async {
                tokio::task::spawn_blocking(|| {
                    (config::load(), git::available(), rebuild::Tools::detect())
                })
                .await
                .unwrap_or((Err(LoadError), false, rebuild::Tools::default()))
            },
            |(result, git_available, rebuild_tools)| {
                Message::Loaded(result, git_available, rebuild_tools)
            },
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
        let inspect = if self.can_inspect_repository() {
            time::every(Duration::from_secs(3)).map(|_| Message::InspectRepository)
        } else {
            Subscription::none()
        };
        Subscription::batch([
            window::close_requests().map(Message::WindowCloseRequested),
            inspect,
        ])
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::WindowCloseRequested(id) => {
                if !self.rebuild.as_ref().is_some_and(rebuild::Run::is_running) {
                    return window::close(id);
                }
                self.pending_close = Some(id);
                self.interrupt_rebuild();
            }
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
                if !self.is_busy() {
                    self.mode = mode;
                }
            }
            Message::Overview(overview::Message::Download) => {
                if !self.git_available || self.is_busy() || !matches!(self.screen, Screen::Overview)
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
                    self.overview.set_download_error(&self.i18n, &self.download);
                    self.working_copy.matches_revision = false;
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
                    self.mode = Mode::Test;
                    self.working_copy = git::WorkingCopy {
                        has_local_changes: false,
                        matches_revision: true,
                    };
                } else if matches!(result, Err(git::Error::LocalChanges)) {
                    self.working_copy.has_local_changes = true;
                }
                self.download = overview::DownloadStatus::Finished(result);
                self.overview.set_download_error(&self.i18n, &self.download);
                return self.inspect_repository();
            }
            Message::Overview(overview::Message::Rebuild) => return self.start_rebuild(),
            Message::Overview(overview::Message::InterruptRebuild) => {
                self.interrupt_rebuild();
            }
            Message::Overview(overview::Message::ToggleJournal) => {
                self.overview.journal_expanded = !self.overview.journal_expanded;
            }
            Message::Overview(overview::Message::SelectText(target, action)) => {
                self.overview.update_selection(target, action);
            }
            Message::Rebuild(event) => {
                if let Some(run) = self.rebuild.as_mut().filter(|run| run.is_running()) {
                    match event {
                        rebuild::Event::Inspected(status) => self.working_copy = status,
                        rebuild::Event::Lines(lines) => {
                            let removed_bytes = run.append(lines);
                            self.overview.update_journal(run, removed_bytes);
                        }
                        rebuild::Event::InterruptFailed => {
                            run.interrupting = false;
                            self.pending_close = None;
                        }
                        rebuild::Event::Finished(outcome) => {
                            run.outcome = Some(outcome);
                            self.rebuild_interrupt = None;
                            if let Some(id) = self.pending_close.take() {
                                return window::close(id);
                            }
                            self.overview.set_rebuild_error(&self.i18n, Some(run));
                            return self.inspect_repository();
                        }
                    }
                }
            }
            Message::InspectRepository => return self.inspect_repository(),
            Message::RepositoryInspected(generation, status) => {
                if generation == self.repository_generation {
                    self.checking_repository = false;
                    self.working_copy = status;
                }
            }
            Message::Loaded(result, git_available, rebuild_tools) => {
                self.loading = false;
                self.git_available = git_available;
                self.rebuild_tools = rebuild_tools;
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
                let can_save = !self.is_busy();
                if let Screen::Settings(state) = &mut self.screen
                    && let Some(draft) = state.update(message, can_save)
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
                    if self.config.as_ref().is_some_and(|previous| {
                        previous.path != config.path
                            || previous.repository != config.repository
                            || previous.branch != config.branch
                            || previous.flake_directory != config.flake_directory
                    }) {
                        self.rebuild = None;
                        self.overview.journal = Default::default();
                    }
                    self.config = Some(config);
                    self.load_error = false;
                    self.screen = Screen::Overview;
                    self.download = overview::DownloadStatus::Idle;
                    self.overview.set_download_error(&self.i18n, &self.download);
                    self.overview
                        .set_rebuild_error(&self.i18n, self.rebuild.as_ref());
                    self.working_copy = git::WorkingCopy::default();
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

    fn is_busy(&self) -> bool {
        self.download.is_running() || self.rebuild.as_ref().is_some_and(rebuild::Run::is_running)
    }

    fn interrupt_rebuild(&mut self) {
        if let (Some(run), Some(interrupt)) = (&mut self.rebuild, &self.rebuild_interrupt)
            && run.is_running()
            && !run.interrupting
            && interrupt.try_send(()).is_ok()
        {
            run.interrupting = true;
        }
    }

    fn start_rebuild(&mut self) -> Task<Message> {
        if self.is_busy()
            || !matches!(self.screen, Screen::Overview)
            || !self.rebuild_tools.available()
            || !self.working_copy.matches_revision
        {
            return Task::none();
        }
        let Some(config) = &self.config else {
            return Task::none();
        };
        let path = config.path.clone();
        let repository = config.repository.clone();
        let branch = config.branch.clone();
        let flake_directory = config.flake_directory.clone();
        let mode = self.mode;
        let (interrupt_request, interrupt) = mpsc::channel(1);
        self.rebuild_interrupt = Some(interrupt_request);
        self.rebuild = Some(rebuild::Run::new(mode));
        self.overview.journal_expanded = true;
        self.overview.journal = Default::default();
        self.overview.set_rebuild_error(&self.i18n, None);
        self.invalidate_repository_check();
        Task::run(
            stream::channel(16, async move |mut sender| {
                let outcome = rebuild::run(
                    path,
                    repository,
                    branch,
                    flake_directory,
                    mode,
                    interrupt,
                    |event| {
                        let mut sender = sender.clone();
                        async move {
                            let _ = sender.send(event).await;
                        }
                    },
                )
                .await;
                let _ = sender.send(rebuild::Event::Finished(outcome)).await;
            }),
            Message::Rebuild,
        )
    }

    fn can_inspect_repository(&self) -> bool {
        matches!(self.screen, Screen::Overview)
            && self.git_available
            && !self.is_busy()
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
        let branch = config.branch.clone();
        let generation = self.repository_generation;
        self.checking_repository = true;
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    git::inspect(&path, &repository, &branch).unwrap_or_default()
                })
                .await
                .unwrap_or_default()
            },
            move |status| Message::RepositoryInspected(generation, status),
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
                            rebuild_tools: self.rebuild_tools,
                            working_copy: self.working_copy,
                            busy: self.is_busy(),
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
                        self.rebuild.as_ref(),
                        &self.overview,
                    )
                    .map(Message::Overview),
                )
            }
            Screen::Settings(state) => (
                self.i18n.text("app-open-settings"),
                self.i18n.text("app-back"),
                (!state.saving()).then_some(Message::CloseSettings),
                state
                    .view(&self.i18n, !self.is_busy())
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
