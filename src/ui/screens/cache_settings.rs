use chrono::{DateTime, Utc};

use crate::attic::{self, Connection, Failure, Metadata, Token, TokenError, ValidationError};

mod view;

#[cfg(test)]
mod tests;

pub const GUIDE_URL: &str = "https://docs.attic.rs/user-guide/index.html#disabling-a-cache";

#[derive(Clone)]
pub enum Message {
    UrlChanged(String),
    ServerNameChanged(String),
    TokenChanged(String),
    CacheSelected(String),
    Connect,
    Forget,
    OpenGuide,
    Close,
}

pub enum Action {
    None,
    Close,
    Connect(Connection),
    Forget,
    OpenGuide,
}

enum Status {
    Idle,
    Connecting,
    Forgetting,
    Connected,
    Forgotten,
    Invalid(ValidationError),
    ConnectFailed(Failure),
    ForgetFailed,
}

pub struct State {
    cli_available: bool,
    url: String,
    server_name: String,
    token: String,
    token_info: Option<Result<Token, TokenError>>,
    cache_name: Option<String>,
    status: Status,
    close_requested: bool,
    guide_failed: bool,
}

impl State {
    pub fn new(saved: Option<&Metadata>, cli_available: bool) -> Self {
        Self {
            cli_available,
            url: saved.map_or_else(String::new, |saved| saved.server_url.clone()),
            server_name: saved.map_or_else(
                || attic::DEFAULT_SERVER_NAME.to_owned(),
                |saved| saved.server_name.clone(),
            ),
            token: String::new(),
            token_info: None,
            cache_name: saved.map(|saved| saved.cache_name.clone()),
            status: Status::Idle,
            close_requested: false,
            guide_failed: false,
        }
    }

    pub fn busy(&self) -> bool {
        matches!(self.status, Status::Connecting | Status::Forgetting)
    }

    pub fn update(&mut self, message: Message, now: DateTime<Utc>) -> Action {
        if matches!(message, Message::Close) {
            if self.busy() {
                self.close_requested = true;
                return Action::None;
            }
            return Action::Close;
        }
        if self.busy() {
            return Action::None;
        }
        match message {
            Message::Connect => {
                if !self.cli_available {
                    return Action::None;
                }
                return match Connection::new(
                    &self.url,
                    &self.server_name,
                    &self.token,
                    self.cache_name.as_deref(),
                    now,
                ) {
                    Ok(connection) => {
                        self.status = Status::Connecting;
                        Action::Connect(connection)
                    }
                    Err(error) => {
                        self.status = Status::Invalid(error);
                        self.refresh_token(now);
                        Action::None
                    }
                };
            }
            Message::Forget => {
                self.status = Status::Forgetting;
                return Action::Forget;
            }
            Message::OpenGuide => {
                self.guide_failed = false;
                return Action::OpenGuide;
            }
            Message::UrlChanged(value) => self.url = value,
            Message::ServerNameChanged(value) => self.server_name = value,
            Message::TokenChanged(value) => self.token = value,
            Message::CacheSelected(name) => self.cache_name = Some(name),
            Message::Close => unreachable!(),
        }
        if !matches!(&self.status, Status::ConnectFailed(error) if error.may_have_applied()) {
            self.status = Status::Idle;
        }
        self.refresh_token(now);
        Action::None
    }

    fn refresh_token(&mut self, now: DateTime<Utc>) {
        let token = self.token.trim();
        self.token_info = (!token.is_empty()).then(|| Token::parse(token, now));
        if let Some(Ok(token)) = &self.token_info
            && let Some(name) = &self.cache_name
            && !token.cache_names.contains(name)
        {
            self.cache_name = None;
        }
    }

    pub fn connected(&mut self, result: &Result<Metadata, Failure>, now: DateTime<Utc>) -> bool {
        let close_requested = std::mem::take(&mut self.close_requested);
        match result {
            Ok(metadata) => {
                self.url.clone_from(&metadata.server_url);
                self.server_name.clone_from(&metadata.server_name);
                self.cache_name = Some(metadata.cache_name.clone());
                self.token.clear();
                self.token_info = None;
                self.status = Status::Connected;
                close_requested
            }
            Err(error) => {
                self.status = Status::ConnectFailed(*error);
                self.refresh_token(now);
                false
            }
        }
    }

    pub fn forgotten(&mut self, succeeded: bool) -> bool {
        let close_requested = std::mem::take(&mut self.close_requested);
        if succeeded {
            *self = Self::new(None, self.cli_available);
            self.status = Status::Forgotten;
            close_requested
        } else {
            self.status = Status::ForgetFailed;
            false
        }
    }

    pub fn guide_finished(&mut self, succeeded: bool) {
        self.guide_failed = !succeeded;
    }
}
