use iced::Task;

use crate::{
    attic,
    ui::screens::{Screen, cache_settings},
};

use super::{App, Message};

impl App {
    pub(super) fn set_cache(
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

    pub(super) fn update_cache_settings(
        &mut self,
        message: cache_settings::Message,
    ) -> Task<Message> {
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

    pub(super) fn cache_connected(
        &mut self,
        result: Result<attic::Metadata, attic::Failure>,
    ) -> Task<Message> {
        if let Screen::CacheSettings(state) = &mut self.screen {
            let close = state.connected(&result, attic::now());
            if let Ok(metadata) = result {
                self.set_cache(Ok(Some(metadata)), attic::now());
                self.overview.journal_expanded = false;
                return self.finish_cache_action(close);
            }
            self.pending_close = None;
        }
        Task::none()
    }

    pub(super) fn cache_forgotten(&mut self, succeeded: bool) -> Task<Message> {
        if let Screen::CacheSettings(state) = &mut self.screen {
            let close = state.forgotten(succeeded);
            if succeeded {
                self.set_cache(Ok(None), attic::now());
                return self.finish_cache_action(close);
            }
            self.pending_close = None;
        }
        Task::none()
    }

    pub(super) fn cache_guide_opened(&mut self, succeeded: bool) {
        if let Screen::CacheSettings(state) = &mut self.screen {
            state.guide_finished(succeeded);
        }
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
}
