use std::{env, path::PathBuf, sync::Arc, time::Duration};

use iced::{
    Element, Event, Subscription, Task, Theme, clipboard, event,
    futures::{FutureExt, SinkExt, stream::unfold},
    keyboard, stream,
    widget::{column, operation},
    window,
};
use tokio::sync::mpsc;

use crate::{
    attic,
    command_runner::{CommandRunner, LogLine},
    config::{self, Config, DEFAULT_UI_SCALE, LoadError, SaveError},
    git,
    i18n::{I18n, LanguagePreference},
    rebuild::{self, history},
    ui::{
        modal,
        screens::{Screen, build_history, cache_settings, overview, settings},
        style, toolbar,
    },
};

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
    history: Option<history::Snapshot>,
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
    HistoryLoaded(u64, history::Snapshot),
    HistoryRemoved(Result<(), rebuild::Error>),
    Overview(overview::Message),
    CacheSettings(cache_settings::Message),
    Settings(settings::Message),
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
            Message::HistoryLoaded(generation, snapshot)
                if generation == self.history_generation =>
            {
                if !snapshot.unavailable
                    && self
                        .built
                        .as_ref()
                        .is_some_and(|build| !snapshot.contains_build(build))
                {
                    self.built = None;
                }
                if let Screen::BuildHistory(state) = &mut self.screen {
                    state.set_snapshot(&self.i18n, &snapshot);
                }
                self.history = Some(snapshot);
                self.history_refreshing = false;
            }
            Message::HistoryRemoved(result) => {
                if let Screen::BuildHistory(state) = &mut self.screen {
                    state.removal_finished(result);
                    let refresh = self.refresh_history();
                    return Task::batch([refresh, self.finish_pending_close()]);
                }
            }
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
                self.screen = Screen::Settings(Box::new(settings::State::new(
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
                    self.overview.journal_expanded = true;
                    self.overview.set_download_error(&self.i18n, &self.download);
                    self.working_copy.matches_revision = false;
                    self.invalidate_repository_check();
                    let runner = self.commands.clone().with_secret(config.token.clone());
                    return Task::perform(
                        async move { git::download(&runner, &config).await },
                        Message::Downloaded,
                    );
                }
            }
            Message::Downloaded(result) => {
                let result = result
                    .map(|status| {
                        self.steps = rebuild::Steps::default();
                        self.working_copy = status;
                        self.overview.journal_expanded = false;
                    })
                    .inspect_err(|error| {
                        if matches!(error, git::Error::LocalChanges) {
                            self.working_copy.has_local_changes = true;
                        }
                    });
                self.refresh_repository_status();
                self.download = overview::DownloadStatus::Finished(result);
                self.overview.set_download_error(&self.i18n, &self.download);
                if let Screen::Settings(state) = &mut self.screen {
                    state.refresh_local_branches();
                    return self.lookup_settings_branches(None);
                }
                return self.inspect_repository();
            }
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
                self.overview.update_selection(target, action);
            }
            Message::CommandLogs(lines) => {
                let generation = self.commands.generation();
                self.overview.append_journal(
                    lines
                        .into_iter()
                        .filter(|line| line.generation == generation),
                );
            }
            Message::Rebuild(event) => {
                if let Some(run) = self.rebuild.as_mut().filter(|run| run.is_running()) {
                    match event {
                        rebuild::Event::Inspected(status) => {
                            self.working_copy = status;
                            self.refresh_repository_status();
                        }
                        rebuild::Event::Started(step) => run.step = step,
                        rebuild::Event::Built(build) => self.built = Some(build),
                        rebuild::Event::InterruptFailed => {
                            run.interrupting = false;
                            if !matches!(&self.screen, Screen::CacheSettings(state) if state.busy())
                            {
                                self.pending_close = None;
                            }
                        }
                        rebuild::Event::Finished(outcome) => {
                            if outcome.is_ok() {
                                self.overview.journal_expanded = false;
                            }
                            if !run.ready && matches!(outcome, Err(rebuild::Error::BuildResult)) {
                                self.built = None;
                            }
                            run.outcome = Some(outcome);
                            self.rebuild_interrupt = None;
                            if !matches!(&self.screen, Screen::CacheSettings(state) if state.busy())
                                && let Some(id) = self.pending_close.take()
                            {
                                return window::close(id);
                            }
                            self.overview.set_rebuild_error(&self.i18n, Some(run));
                            let repository = !run.ready;
                            let history = self.refresh_history();
                            return if repository {
                                Task::batch([history, self.inspect_repository()])
                            } else {
                                history
                            };
                        }
                    }
                }
            }
            Message::RepositoryInspected(generation, status) => {
                if generation == self.repository_generation {
                    self.checking_repository = false;
                    self.working_copy = status;
                    self.refresh_repository_status();
                }
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
            Message::CacheConnected(result) => {
                if let Screen::CacheSettings(state) = &mut self.screen {
                    let close = state.connected(&result, attic::now());
                    if let Ok(metadata) = result {
                        self.set_cache(Ok(Some(metadata)), attic::now());
                        return self.finish_cache_action(close);
                    }
                    self.pending_close = None;
                }
            }
            Message::CacheForgotten(succeeded) => {
                if let Screen::CacheSettings(state) = &mut self.screen {
                    let close = state.forgotten(succeeded);
                    if succeeded {
                        self.set_cache(Ok(None), attic::now());
                        return self.finish_cache_action(close);
                    }
                    self.pending_close = None;
                }
            }
            Message::CacheGuideOpened(succeeded) => {
                if let Screen::CacheSettings(state) = &mut self.screen {
                    state.guide_finished(succeeded);
                }
            }
            Message::Settings(message) => {
                let can_save = !self.is_busy();
                if let Screen::Settings(state) = &mut self.screen {
                    match state.update(message, can_save) {
                        settings::Action::None => {}
                        settings::Action::Close => {
                            let language = self.config.as_ref().map(|config| config.language);
                            self.set_language(language.unwrap_or_default());
                            self.screen = Screen::Overview;
                            return self.inspect_repository();
                        }
                        settings::Action::LookupBranches(remote) => {
                            return self.lookup_settings_branches(remote);
                        }
                        settings::Action::PreviewLanguage(language) => self.set_language(language),
                        settings::Action::Save(draft) => return self.save_settings(draft),
                    }
                }
            }
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
                    return self.update(Message::Settings(settings::Message::Cancel));
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
                if let Screen::Settings(state) = &mut self.screen {
                    state.local_branches_finished(generation, branches);
                }
            }
            Message::SettingsRemoteRefs(generation, result) => {
                let can_save = !self.is_busy();
                if let Screen::Settings(state) = &mut self.screen
                    && let Some(draft) = state.remote_lookup_finished(generation, result, can_save)
                {
                    return self.save_settings(draft);
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
                        self.built = None;
                        self.clear_journal();
                    }
                    self.config = Some(config);
                    self.load_error = false;
                    self.screen = Screen::Overview;
                    self.download = overview::DownloadStatus::Idle;
                    self.overview.set_download_error(&self.i18n, &self.download);
                    self.overview
                        .set_rebuild_error(&self.i18n, self.rebuild.as_ref());
                    self.working_copy = git::WorkingCopy::default();
                    self.refresh_repository_status();
                    self.invalidate_repository_check();
                    return self.inspect_repository();
                }
                Err(error) => {
                    if let Screen::Settings(state) = &mut self.screen {
                        state.save_failed(error);
                    }
                }
            },
            Message::OpenSettingsMenu
            | Message::OpenSettings
            | Message::OpenCacheSettings
            | Message::Overview(overview::Message::OpenActionsMenu)
            | Message::Overview(overview::Message::OpenBuildHistory)
            | Message::HistoryLoaded(_, _) => {}
        }
        Task::none()
    }

    fn set_language(&mut self, language: LanguagePreference) {
        if self.i18n.set_language(language) {
            self.refresh_repository_status();
            self.overview.set_download_error(&self.i18n, &self.download);
            self.overview
                .set_rebuild_error(&self.i18n, self.rebuild.as_ref());
            if let (Screen::BuildHistory(state), Some(snapshot)) = (&mut self.screen, &self.history)
            {
                state.set_snapshot(&self.i18n, snapshot);
            }
        }
    }

    fn refresh_repository_status(&mut self) {
        if let Some(config) = &self.config {
            self.overview
                .set_repository_status(&self.i18n, &self.working_copy, &config.branch);
        }
    }

    fn is_busy(&self) -> bool {
        self.download.is_running()
            || self.rebuild.as_ref().is_some_and(rebuild::Run::is_running)
            || matches!(&self.screen, Screen::BuildHistory(state) if state.removing())
    }

    fn set_cache(
        &mut self,
        result: Result<Option<attic::Metadata>, attic::LoadError>,
        now: chrono::DateTime<chrono::Utc>,
    ) {
        self.cache_expiration_warning = result
            .as_ref()
            .ok()
            .and_then(Option::as_ref)
            .and_then(|metadata| metadata.warning(now));
        self.cache = result;
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

    fn finish_cache_action(&mut self, close_modal: bool) -> Task<Message> {
        if close_modal {
            self.screen = Screen::Overview;
        }
        if self.pending_close.is_some() {
            return self.finish_pending_close();
        }
        self.inspect_repository()
    }

    fn update_cache_settings(&mut self, message: cache_settings::Message) -> Task<Message> {
        let Screen::CacheSettings(state) = &mut self.screen else {
            return Task::none();
        };
        match state.update(message, attic::now()) {
            cache_settings::Action::None => Task::none(),
            cache_settings::Action::Close => {
                self.screen = Screen::Overview;
                self.inspect_repository()
            }
            cache_settings::Action::Connect(connection) => {
                self.overview.journal_expanded = true;
                let home = self.home.clone();
                let runner = self.commands.clone();
                Task::perform(
                    async move { attic::connect(home.as_deref(), runner, connection).await },
                    Message::CacheConnected,
                )
            }
            cache_settings::Action::Forget => {
                let home = self.home.clone();
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || attic::forget(home.as_deref()))
                            .await
                            .is_ok_and(|result| result.is_ok())
                    },
                    Message::CacheForgotten,
                )
            }
            cache_settings::Action::OpenGuide => Task::perform(
                async {
                    tokio::process::Command::new("xdg-open")
                        .arg(cache_settings::GUIDE_URL)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status()
                        .await
                        .is_ok_and(|status| status.success())
                },
                Message::CacheGuideOpened,
            ),
        }
    }

    fn can_activate_history(&self) -> bool {
        !self.is_busy()
            && self
                .history
                .as_ref()
                .is_some_and(|snapshot| !snapshot.unavailable)
            && self
                .rebuild_tools
                .missing(false, rebuild::Steps::ACTIVATE)
                .next()
                .is_none()
    }

    fn refresh_history(&mut self) -> Task<Message> {
        if self.history_refreshing {
            return Task::none();
        }
        self.history_refreshing = true;
        self.history = None;
        self.history_generation += 1;
        let generation = self.history_generation;
        Task::perform(
            async {
                tokio::task::spawn_blocking(history::snapshot)
                    .await
                    .unwrap_or_else(|_| {
                        let mut snapshot = history::Snapshot::default();
                        snapshot.unavailable = true;
                        snapshot
                    })
            },
            move |snapshot| Message::HistoryLoaded(generation, snapshot),
        )
    }

    fn update_history(&mut self, message: build_history::Message) -> Task<Message> {
        let can_activate = self.can_activate_history();
        let can_remove_generations = self
            .rebuild_tools
            .missing_for_generation_removal()
            .is_none();
        let Screen::BuildHistory(state) = &mut self.screen else {
            return Task::none();
        };
        match state.update(
            message,
            self.history.as_ref(),
            can_activate,
            can_remove_generations,
        ) {
            build_history::Action::None => Task::none(),
            build_history::Action::Close => {
                self.screen = Screen::Overview;
                Task::none()
            }
            build_history::Action::Refresh => self.refresh_history(),
            build_history::Action::Activate(target) => {
                self.screen = Screen::Overview;
                self.start_run(
                    rebuild::Request::Ready(target),
                    rebuild::Steps::ACTIVATE,
                    true,
                )
            }
            build_history::Action::Remove(marked) => {
                let runner = self.commands.clone();
                Task::perform(history::remove(runner, marked), Message::HistoryRemoved)
            }
        }
    }

    fn lookup_settings_branches(&mut self, remote: Option<settings::RemoteQuery>) -> Task<Message> {
        let Screen::Settings(state) = &mut self.screen else {
            return Task::none();
        };
        let Some(lookup) = state.branch_lookup(remote) else {
            return Task::none();
        };
        self.settings_lookup_generation += 1;
        let generation = self.settings_lookup_generation;
        let runner = self.commands.clone().with_secret(lookup.token.clone());
        let path = self
            .home
            .as_deref()
            .and_then(|home| config::absolute_repo_path(&lookup.path, home).ok());
        // Both lookups use the same origin check before reading the working copy.
        let worktree = {
            let runner = runner.clone();
            let repository = lookup.repository.clone();
            async move {
                let path = path?;
                git::check_worktree(&runner, &path, &repository)
                    .await
                    .ok()?;
                Some(path)
            }
            .shared()
        };
        let mut tasks = Vec::new();
        if lookup.check_local {
            let runner = runner.clone();
            let worktree = worktree.clone();
            let (task, handle) = Task::perform(
                async move {
                    match worktree.await {
                        Some(path) => git::read_branches(&runner, &path).await.unwrap_or_default(),
                        None => Vec::new(),
                    }
                },
                move |branches| Message::SettingsLocalBranches(generation, branches),
            )
            .abortable();
            state.local_branches_started(generation, handle);
            tasks.push(task);
        }
        if let Some(query) = lookup.remote {
            let (task, handle) = Task::perform(
                async move {
                    match query {
                        settings::RemoteQuery::Head => {
                            git::remote_head(&runner, &lookup.repository, &lookup.token).await
                        }
                        settings::RemoteQuery::Branches => {
                            let worktree = worktree.await;
                            git::remote_branches(
                                &runner,
                                &lookup.repository,
                                &lookup.token,
                                worktree.as_deref(),
                            )
                            .await
                        }
                    }
                },
                move |result| Message::SettingsRemoteRefs(generation, result),
            )
            .abortable();
            state.remote_lookup_started(generation, query, handle);
            tasks.push(task);
        }
        Task::batch(tasks)
    }

    fn save_settings(&self, draft: Config) -> Task<Message> {
        let repository_required = self
            .config
            .as_ref()
            .and_then(Config::repository_path)
            .is_some();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || config::save(draft, repository_required))
                    .await
                    .unwrap_or(Err(SaveError::Write))
            },
            Message::Saved,
        )
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
        let cached = self.current_build().cloned();
        let needs_build = cached.is_none();
        let steps = self.steps;
        if self.is_busy()
            || !matches!(self.screen, Screen::Overview)
            || !self.working_copy.matches_revision
            || (!needs_build && !steps.applies())
            || self
                .rebuild_tools
                .missing(needs_build, steps)
                .next()
                .is_some()
        {
            return Task::none();
        }
        let Some(config) = self.config.clone() else {
            return Task::none();
        };
        self.start_run(
            rebuild::Request::Repository {
                config: Box::new(config),
                steps,
                cached,
            },
            steps,
            needs_build,
        )
    }

    fn start_run(
        &mut self,
        request: rebuild::Request,
        steps: rebuild::Steps,
        clear: bool,
    ) -> Task<Message> {
        let (interrupt_request, interrupt) = mpsc::channel(1);
        self.rebuild_interrupt = Some(interrupt_request);
        let ready = matches!(&request, rebuild::Request::Ready(_));
        self.rebuild = Some(rebuild::Run::new(steps, ready));
        self.history_generation += 1;
        self.history_refreshing = false;
        self.overview.journal_expanded = true;
        if clear {
            self.clear_journal();
        }
        self.overview.set_rebuild_error(&self.i18n, None);
        let mut runner = self.commands.clone();
        if let rebuild::Request::Repository { config, .. } = &request {
            self.invalidate_repository_check();
            runner = runner.with_secret(config.token.clone());
        }
        Task::run(
            stream::channel(16, async move |mut sender| {
                let outcome = rebuild::run(&runner, request, interrupt, |event| {
                    let mut sender = sender.clone();
                    async move {
                        let _ = sender.send(event).await;
                    }
                })
                .await;
                let _ = sender.send(rebuild::Event::Finished(outcome)).await;
            }),
            Message::Rebuild,
        )
    }

    fn current_build(&self) -> Option<&Arc<rebuild::Build>> {
        self.built
            .as_ref()
            .filter(|build| build.matches_worktree(&self.working_copy))
    }

    fn inspect_repository(&mut self) -> Task<Message> {
        if self.checking_repository
            || !self.git_available
            || self.is_busy()
            || !matches!(self.screen, Screen::Overview)
        {
            return Task::none();
        }
        let Some(config) = self
            .config
            .as_ref()
            .filter(|config| config.repository_path().is_some())
            .cloned()
        else {
            return Task::none();
        };
        let generation = self.repository_generation;
        let runner = self.commands.clone().with_secret(config.token.clone());
        self.checking_repository = true;
        Task::perform(
            async move { git::inspect(&runner, &config).await.unwrap_or_default() },
            move |status| Message::RepositoryInspected(generation, status),
        )
    }

    fn invalidate_repository_check(&mut self) {
        self.repository_generation += 1;
        self.checking_repository = false;
    }

    fn clear_journal(&mut self) {
        self.commands.next_generation();
        self.overview.clear_journal();
    }

    pub fn view(&self) -> Element<'_, Message> {
        let dialog = match &self.screen {
            Screen::Overview => None,
            Screen::BuildHistory(state) => Some(
                state
                    .view(
                        &self.i18n,
                        self.history.as_ref(),
                        self.can_activate_history(),
                        self.rebuild_tools
                            .missing(false, rebuild::Steps::ACTIVATE)
                            .next(),
                        self.rebuild_tools.missing_for_generation_removal(),
                    )
                    .map(Message::BuildHistory),
            ),
            Screen::CacheSettings(state) => Some(
                state
                    .view(&self.i18n, &self.cache)
                    .map(Message::CacheSettings),
            ),
            Screen::Settings(state) => Some(
                state
                    .view(&self.i18n, !self.is_busy())
                    .map(Message::Settings),
            ),
        };
        modal::over(self.overview_view(), dialog)
    }

    fn overview_view(&self) -> Element<'_, Message> {
        let repository = self.config.as_ref().and_then(|config| {
            config.repository_path().map(|path| overview::Repository {
                path: config::display_repo_path(path, self.home.as_deref()),
                git_available: self.git_available,
                rebuild_tools: self.rebuild_tools,
                working_copy: &self.working_copy,
                steps: self.steps,
                needs_build: self.current_build().is_none(),
                busy: self.is_busy(),
                download: &self.download,
            })
        });
        let content = overview::view(
            &self.i18n,
            repository,
            self.loading,
            self.load_error,
            match (&self.cache, self.cache_expiration_warning) {
                (Err(_), _) => Some(overview::CacheNotice::LoadError),
                (Ok(Some(metadata)), Some(warning)) => {
                    Some(overview::CacheNotice::Expiration(metadata, warning))
                }
                _ => None,
            },
            self.rebuild.as_ref(),
            &self.overview,
        )
        .map(Message::Overview);
        column![
            toolbar::view(
                self.title(),
                (!self.loading).then_some(Message::OpenSettingsMenu),
                self.settings_menu_open.then(|| {
                    [
                        (self.i18n.text("app-open-settings"), Message::OpenSettings),
                        (
                            self.i18n.text("app-open-cache-settings"),
                            Message::OpenCacheSettings,
                        ),
                    ]
                }),
                Message::DismissDialog,
            ),
            content,
        ]
        .into()
    }
}
