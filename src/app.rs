use std::{env, path::PathBuf};

use iced::{
    Center, Element, Fill, Task, Theme,
    widget::{button, column, container, row, space, text},
};

use crate::{
    config::{self, Config, LoadError, SaveError},
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
}

#[derive(Clone)]
pub enum Message {
    OpenSettings,
    CloseSettings,
    Overview(overview::Message),
    Settings(settings::Message),
    Loaded(Result<Option<Config>, LoadError>),
    Saved(Result<Config, SaveError>),
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
        };
        let task = Task::perform(
            async {
                tokio::task::spawn_blocking(config::load)
                    .await
                    .unwrap_or(Err(LoadError))
            },
            Message::Loaded,
        );
        (app, task)
    }

    pub fn title(&self) -> String {
        self.i18n.text("app-title")
    }

    pub fn theme(&self) -> Theme {
        self.theme.clone()
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
                }
            }
            Message::Overview(overview::Message::SelectMode(mode)) => {
                self.mode = mode;
            }
            Message::Loaded(result) => {
                self.loading = false;
                match result {
                    Ok(Some(config)) => {
                        self.i18n.set_language(config.language);
                        self.config = Some(config);
                    }
                    Ok(None) => {}
                    Err(LoadError) => self.load_error = true,
                }
            }
            Message::Settings(message) => {
                if let Screen::Settings(state) = &mut self.screen
                    && let Some(draft) = state.update(message)
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

    pub fn view(&self) -> Element<'_, Message> {
        let (title, action, navigation, content) = match &self.screen {
            Screen::Overview => {
                let path = self
                    .config
                    .as_ref()
                    .and_then(Config::repository_path)
                    .map(|path| config::display_repo_path(path, self.home.as_deref()));
                (
                    self.title(),
                    self.i18n.text("app-open-settings"),
                    (!self.loading).then_some(Message::OpenSettings),
                    overview::view(
                        &self.i18n,
                        path.as_deref(),
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
                state.view(&self.i18n).map(Message::Settings),
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
