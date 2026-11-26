use std::{env, path::PathBuf, sync::Arc, time::Duration};

use iced::{
    Event, Subscription, Task, Theme, clipboard, event, futures::stream::unfold, keyboard,
    widget::operation, window,
};
use tokio::sync::mpsc;

use crate::{
    attic,
    command_runner::{CommandRunner, LogLine},
    config::{self, Config, DEFAULT_UI_SCALE, LoadError, SaveError},
    git,
    i18n::I18n,
    rebuild::{self, history::Snapshot},
    ui::{
        screens::{Screen, build_history, cache_settings, overview, settings as settings_screen},
        style,
    },
};

mod build;
mod cache;
mod history;
mod repository;
mod settings;
mod view;

#[cfg(test)]
mod tests;

pub struct App {
    screen: Screen,
    settings_menu_open: bool,
    i18n: I18n,
    theme: Theme,
    home: Option<PathBuf>,
    config: Option<Config>,
    cache: Result<Option<attic::Metadata>, attic::LoadError>,
    cache_expiration_warning: Option<attic::ExpirationWarning>,
    loading: bool,
    load_error: bool,
    steps: rebuild::Steps,
    git_available: bool,
    rebuild_tools: rebuild::Tools,
    working_copy: git::WorkingCopy,
    download: overview::DownloadStatus,
    rebuild: Option<rebuild::Run>,
    built: Option<Arc<rebuild::Build>>,
    history: Option<Snapshot>,
    history_generation: u64,
    history_refreshing: bool,
    rebuild_interrupt: Option<mpsc::Sender<()>>,
    pending_close: Option<window::Id>,
    overview: overview::State,
    commands: CommandRunner,
    checking_repository: bool,
    repository_generation: u64,
    settings_lookup_generation: u64,
}

#[derive(Clone)]
pub enum Message {
    WindowCloseRequested(window::Id),
    OpenSettingsMenu,
    OpenSettings,
    OpenCacheSettings,
    BuildHistory(build_history::Message),
    HistoryLoaded(u64, Snapshot),
    HistoryRemoved(Result<(), rebuild::Error>),
    Overview(overview::Message),
    CacheSettings(cache_settings::Message),
    Settings(settings_screen::Message),
    FocusDialogInput(bool),
    DismissDialog,
    SettingsLocalBranches(u64, Vec<git::Branch>),
    SettingsRemoteRefs(u64, Result<git::RemoteRefs, git::RemoteBranchesError>),
    Loaded(
        Result<Option<Config>, LoadError>,
        Result<Option<attic::Metadata>, attic::LoadError>,
        bool,
        rebuild::Tools,
    ),
    Saved(Result<Config, SaveError>),
    CacheConnected(Result<attic::Metadata, attic::Failure>),
    CacheForgotten(bool),
    CacheGuideOpened(bool),
    Downloaded(Result<git::WorkingCopy, git::Error>),
    CommandLogs(Vec<LogLine>),
    Rebuild(rebuild::Event),
    RepositoryInspected(u64, git::WorkingCopy),
}

impl App {
    pub fn boot() -> (Self, Task<Message>) {
        let (journal, receiver) = mpsc::channel(256);
        let app = Self {
            screen: Screen::Overview,
            settings_menu_open: false,
            i18n: I18n::system(),
            theme: style::theme(),
            home: env::home_dir(),
            config: None,
            cache: Ok(None),
            cache_expiration_warning: None,
            loading: true,
            load_error: false,
            steps: rebuild::Steps::default(),
            git_available: false,
            rebuild_tools: rebuild::Tools::default(),
            working_copy: git::WorkingCopy::default(),
            download: overview::DownloadStatus::Idle,
            rebuild: None,
            built: None,
            history: None,
            history_generation: 0,
            history_refreshing: false,
            rebuild_interrupt: None,
            pending_close: None,
            overview: overview::State::default(),
            commands: CommandRunner::new(journal),
            checking_repository: false,
            repository_generation: 0,
            settings_lookup_generation: 0,
        };
        let runner = app.commands.clone();
        let home = app.home.clone();
        let load = Task::perform(
            async move {
                let (result, cache, rebuild_tools) = tokio::task::spawn_blocking(move || {
                    (
                        config::load(),
                        attic::load(home.as_deref()),
                        rebuild::Tools::detect(),
                    )
                })
                .await
                .unwrap_or((
                    Err(LoadError),
                    Err(attic::LoadError),
                    rebuild::Tools::default(),
                ));
                let runner = match &result {
                    Ok(Some(config)) => runner.with_secret(config.token.clone()),
                    _ => runner,
                };
                let git_available = git::available(&runner).await;
                (result, cache, git_available, rebuild_tools)
            },
            |(result, cache, git_available, rebuild_tools)| {
                Message::Loaded(result, cache, git_available, rebuild_tools)
            },
        );
        let journal = Task::run(
            unfold(receiver, |mut receiver| async move {
                const MAX_BATCH: usize = 1024;
                let mut lines = vec![receiver.recv().await?];
                // Group bursts so the UI can lay out several lines in one update.
                let deadline = tokio::time::Instant::now() + Duration::from_millis(16);
                while lines.len() < MAX_BATCH {
                    let remaining = MAX_BATCH - lines.len();
                    match tokio::time::timeout_at(
                        deadline,
                        receiver.recv_many(&mut lines, remaining),
                    )
                    .await
                    {
                        Ok(count) if count > 0 => {}
                        _ => break,
                    }
                }
                Some((lines, receiver))
            }),
            Message::CommandLogs,
        );
        (app, Task::batch([load, journal]))
    }

    pub fn title(&self) -> String {
        self.i18n.text("app-title")
    }

    pub fn theme(&self) -> Theme {
        self.theme.clone()
    }

    pub fn scale_factor(&self) -> f32 {
        let scale = match &self.screen {
            Screen::Overview | Screen::CacheSettings(_) | Screen::BuildHistory(_) => self
                .config
                .as_ref()
                .map_or(DEFAULT_UI_SCALE, |config| config.ui_scale),
            Screen::Settings(state) => state.ui_scale(),
        };
        f32::from(scale) / 100.0
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let close = window::close_requests().map(Message::WindowCloseRequested);
        let cache = matches!(self.screen, Screen::CacheSettings(_));
        let history = matches!(self.screen, Screen::BuildHistory(_));
        if self.settings_menu_open
            || self.overview.actions_menu_open
            || history
            || cache
            || matches!(&self.screen, Screen::Settings(state) if !state.saving())
        {
            Subscription::batch([
                close,
                event::listen_with(|event, status, _| match (event, status) {
                    (
                        Event::Keyboard(keyboard::Event::KeyPressed {
                            key: keyboard::Key::Named(keyboard::key::Named::Tab),
                            modifiers,
                            ..
                        }),
                        event::Status::Ignored,
                    ) => Some(Message::FocusDialogInput(modifiers.shift())),
                    (
                        Event::Keyboard(keyboard::Event::KeyPressed {
                            key: keyboard::Key::Named(keyboard::key::Named::Escape),
                            repeat: false,
                            ..
                        }),
                        event::Status::Ignored,
                    ) => Some(Message::DismissDialog),
                    _ => None,
                }),
            ])
        } else {
            close
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::WindowCloseRequested(id) => {
                self.pending_close = Some(id);
                return self.finish_pending_close();
            }
            Message::OpenSettingsMenu
                if !self.loading && matches!(self.screen, Screen::Overview) =>
            {
                self.overview.actions_menu_open = false;
                self.settings_menu_open = true;
            }
            Message::Overview(overview::Message::OpenActionsMenu)
                if !self.loading && !self.is_busy() && matches!(self.screen, Screen::Overview) =>
            {
                self.settings_menu_open = false;
                self.overview.actions_menu_open = true;
            }
            Message::Overview(overview::Message::CloseActionsMenu) => {
                self.overview.actions_menu_open = false;
            }
            Message::Overview(overview::Message::OpenBuildHistory)
                if !self.loading && !self.is_busy() && matches!(self.screen, Screen::Overview) =>
            {
                self.settings_menu_open = false;
                self.overview.actions_menu_open = false;
                self.screen = Screen::BuildHistory(build_history::State::default());
                return self.refresh_history();
            }
            Message::BuildHistory(message) => return self.update_history(message),
            Message::HistoryLoaded(generation, snapshot) => {
                self.history_loaded(generation, snapshot)
            }
            Message::HistoryRemoved(result) => return self.history_removed(result),
            Message::OpenCacheSettings
                if !self.loading && matches!(self.screen, Screen::Overview) =>
            {
                self.settings_menu_open = false;
                self.overview.actions_menu_open = false;
                self.screen = Screen::CacheSettings(Box::new(cache_settings::State::new(
                    self.cache.as_ref().ok().and_then(Option::as_ref),
                    attic::available(env::var_os("PATH").as_deref()),
                )));
            }
            Message::OpenSettings if !self.loading && matches!(self.screen, Screen::Overview) => {
                self.settings_menu_open = false;
                self.overview.actions_menu_open = false;
                self.screen = Screen::Settings(Box::new(settings_screen::State::new(
                    self.config.as_ref(),
                    self.home.as_deref(),
                    self.git_available,
                )));
                return self.lookup_settings_branches(None);
            }
            Message::Overview(overview::Message::SetBoot(boot)) => {
                if !self.is_busy() {
                    self.steps.boot = boot;
                }
            }
            Message::Overview(overview::Message::SetActivate(activate)) => {
                if !self.is_busy() {
                    self.steps.activate = activate;
                }
            }
            Message::Overview(overview::Message::Download) => return self.download_repository(),
            Message::Downloaded(result) => return self.download_finished(result),
            Message::Overview(overview::Message::Rebuild) => return self.start_rebuild(),
            Message::Overview(overview::Message::InterruptRebuild) => {
                self.interrupt_rebuild();
            }
            Message::Overview(overview::Message::ToggleJournal) => {
                self.overview.journal_expanded = !self.overview.journal_expanded;
            }
            Message::Overview(overview::Message::CopyJournal) => {
                return self
                    .overview
                    .journal_text()
                    .map_or_else(Task::none, clipboard::write);
            }
            Message::Overview(overview::Message::ClearJournal) => self.overview.clear_journal(),
            Message::Overview(overview::Message::SelectText(target, action)) => {
                return self
                    .overview
                    .update_selection(target, action)
                    .map_or_else(Task::none, clipboard::write_primary);
            }
            Message::CommandLogs(lines) => {
                let generation = self.commands.generation();
                self.overview.append_journal(
                    lines
                        .into_iter()
                        .filter(|line| line.generation == generation),
                );
            }
            Message::Rebuild(event) => return self.update_rebuild(event),
            Message::RepositoryInspected(generation, status) => {
                self.repository_inspected(generation, status);
            }
            Message::Loaded(result, cache, git_available, rebuild_tools) => {
                self.loading = false;
                self.git_available = git_available;
                self.rebuild_tools = rebuild_tools;
                self.set_cache(cache, attic::now());
                match result {
                    Ok(Some(config)) => {
                        self.i18n.set_language(config.language);
                        self.config = Some(config);
                    }
                    Ok(None) => {}
                    Err(LoadError) => self.load_error = true,
                }
                self.refresh_repository_status();
                return Task::batch([self.refresh_history(), self.inspect_repository()]);
            }
            Message::CacheSettings(message) => return self.update_cache_settings(message),
            Message::CacheConnected(result) => return self.cache_connected(result),
            Message::CacheForgotten(succeeded) => return self.cache_forgotten(succeeded),
            Message::CacheGuideOpened(succeeded) => self.cache_guide_opened(succeeded),
            Message::Settings(message) => return self.update_settings(message),
            Message::DismissDialog => match self.screen {
                Screen::Overview => {
                    self.settings_menu_open = false;
                    self.overview.actions_menu_open = false;
                }
                Screen::BuildHistory(_) => {
                    return self.update_history(build_history::Message::Dismiss);
                }
                Screen::CacheSettings(_) => {
                    return self.update_cache_settings(cache_settings::Message::Close);
                }
                Screen::Settings(_) => {
                    return self.update_settings(settings_screen::Message::Cancel);
                }
            },
            Message::FocusDialogInput(reverse) => {
                if matches!(&self.screen, Screen::Settings(state) if !state.saving())
                    || matches!(&self.screen, Screen::CacheSettings(state) if !state.busy())
                    || matches!(self.screen, Screen::BuildHistory(_))
                {
                    return if reverse {
                        operation::focus_previous()
                    } else {
                        operation::focus_next()
                    };
                }
            }
            Message::SettingsLocalBranches(generation, branches) => {
                self.settings_local_branches_loaded(generation, branches);
            }
            Message::SettingsRemoteRefs(generation, result) => {
                return self.settings_remote_refs_loaded(generation, result);
            }
            Message::Saved(result) => return self.settings_saved(result),
            Message::OpenSettingsMenu
            | Message::OpenSettings
            | Message::OpenCacheSettings
            | Message::Overview(overview::Message::OpenActionsMenu)
            | Message::Overview(overview::Message::OpenBuildHistory) => {}
        }
        Task::none()
    }

    fn is_busy(&self) -> bool {
        self.download.is_running()
            || self.rebuild.as_ref().is_some_and(rebuild::Run::is_running)
            || matches!(&self.screen, Screen::BuildHistory(state) if state.removing())
    }

    fn finish_pending_close(&mut self) -> Task<Message> {
        let Some(id) = self.pending_close else {
            return Task::none();
        };
        if matches!(&self.screen, Screen::CacheSettings(state) if state.busy()) {
            return Task::none();
        }
        if matches!(&self.screen, Screen::BuildHistory(state) if state.removing()) {
            return Task::none();
        }
        if self.rebuild.as_ref().is_some_and(rebuild::Run::is_running) {
            self.interrupt_rebuild();
            return Task::none();
        }
        self.pending_close = None;
        window::close(id)
    }

    fn clear_journal(&mut self) {
        self.commands.next_generation();
        self.overview.clear_journal();
    }
}
