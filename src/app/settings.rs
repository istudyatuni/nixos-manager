use iced::{Task, futures::FutureExt};

use crate::{
    config::{self, Config, SaveError},
    git,
    i18n::LanguagePreference,
    ui::screens::{Screen, overview, settings},
};

use super::{App, Message};

impl App {
    pub(super) fn update_settings(&mut self, message: settings::Message) -> Task<Message> {
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
        Task::none()
    }

    pub(super) fn lookup_settings_branches(
        &mut self,
        remote: Option<settings::RemoteQuery>,
    ) -> Task<Message> {
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

    pub(super) fn settings_local_branches_loaded(
        &mut self,
        generation: u64,
        branches: Vec<git::Branch>,
    ) {
        if let Screen::Settings(state) = &mut self.screen {
            state.local_branches_finished(generation, branches);
        }
    }

    pub(super) fn settings_remote_refs_loaded(
        &mut self,
        generation: u64,
        result: Result<git::RemoteRefs, git::RemoteBranchesError>,
    ) -> Task<Message> {
        let can_save = !self.is_busy();
        if let Screen::Settings(state) = &mut self.screen
            && let Some(draft) = state.remote_lookup_finished(generation, result, can_save)
        {
            return self.save_settings(draft);
        }
        Task::none()
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

    pub(super) fn settings_saved(&mut self, result: Result<Config, SaveError>) -> Task<Message> {
        match result {
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
        }
        Task::none()
    }

    pub(super) fn set_language(&mut self, language: LanguagePreference) {
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
}
