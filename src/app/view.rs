use iced::{Element, widget::column};

use crate::{
    config, rebuild,
    ui::{
        modal,
        screens::{Screen, overview},
        toolbar,
    },
};

use super::{App, Message};

impl App {
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
