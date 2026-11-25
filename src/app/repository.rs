use iced::Task;

use crate::{
    git, rebuild,
    ui::screens::{Screen, overview},
};

use super::{App, Message};

impl App {
    pub(super) fn download_repository(&mut self) -> Task<Message> {
        if !self.git_available || self.is_busy() || !matches!(self.screen, Screen::Overview) {
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
        Task::none()
    }

    pub(super) fn download_finished(
        &mut self,
        result: Result<git::WorkingCopy, git::Error>,
    ) -> Task<Message> {
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
        self.inspect_repository()
    }

    pub(super) fn inspect_repository(&mut self) -> Task<Message> {
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

    pub(super) fn repository_inspected(&mut self, generation: u64, status: git::WorkingCopy) {
        if generation == self.repository_generation {
            self.checking_repository = false;
            self.working_copy = status;
            self.refresh_repository_status();
        }
    }

    pub(super) fn invalidate_repository_check(&mut self) {
        self.repository_generation += 1;
        self.checking_repository = false;
    }

    pub(super) fn refresh_repository_status(&mut self) {
        if let Some(config) = &self.config {
            self.overview
                .set_repository_status(&self.i18n, &self.working_copy, &config.branch);
        }
    }
}
