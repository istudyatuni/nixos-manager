use fluent_bundle::FluentArgs;
use iced::{
    Center, Color, Element, Fill,
    widget::{
        Column, Text, button, column, pick_list, rich_text, row, scrollable, space, span, text,
        text_input,
    },
};

use crate::{
    attic::{self, FailureKind, Metadata, Stage, TokenError, ValidationError},
    i18n::I18n,
    ui::{
        modal,
        style::{buttons, colors, inputs, pick_lists, spacing},
    },
};

use super::{GUIDE_URL, Message, State, Status};

impl State {
    pub fn view<'a>(
        &'a self,
        i18n: &'a I18n,
        saved: &'a Result<Option<Metadata>, attic::LoadError>,
    ) -> Element<'a, Message> {
        modal::dialog(move || self.panel(i18n, saved), Some(Message::Close))
    }

    fn panel<'a>(
        &'a self,
        i18n: &'a I18n,
        saved: &'a Result<Option<Metadata>, attic::LoadError>,
    ) -> Element<'a, Message> {
        let metadata = saved.as_ref().ok().and_then(Option::as_ref);
        let header = row![
            text(i18n.text("app-open-cache-settings")).size(22),
            space().width(Fill),
            button(text("×").size(20).center())
                .width(32)
                .height(32)
                .padding(0)
                .style(buttons::secondary)
                .on_press(Message::Close),
        ]
        .align_y(Center)
        .spacing(spacing::LG);

        let mut body = Column::new().spacing(spacing::LG);
        if saved.is_err() {
            body = body.push(note(i18n, "cache-settings-load-error", colors::WARNING));
        }
        let can_connect = if self.cli_available {
            let (form, can_connect) = self.form(i18n, metadata);
            body = body.push(form);
            Some(can_connect)
        } else {
            body = body.push(note(i18n, "cache-settings-missing-cli", colors::WARNING));
            if let Some(metadata) = metadata {
                let mut args = FluentArgs::new();
                args.set("cache", metadata.cache_name.as_str());
                args.set("server", metadata.server_name.as_str());
                args.set("url", metadata.server_url.as_str());
                body =
                    body.push(text(i18n.with_args("cache-settings-stored-cache", &args)).size(14));
            }
            None
        };
        if let Some(metadata) = metadata {
            let mut args = FluentArgs::new();
            args.set("timestamp", metadata.cache_expiration.timestamp());
            body = body.push(text(i18n.with_args("cache-settings-expiration", &args)).size(14));
        }
        if let Some(can_connect) = can_connect {
            body = body.push(self.connection_actions(i18n, metadata.is_some(), can_connect));
        }
        body = body.push(self.forget_section(i18n, !matches!(saved, Ok(None))));

        column![
            header,
            scrollable(body)
                .width(Fill)
                .height(Fill)
                .spacing(spacing::MD),
        ]
        .spacing(spacing::XL)
        .into()
    }

    fn connection_actions<'a>(
        &self,
        i18n: &I18n,
        updating: bool,
        can_connect: bool,
    ) -> Column<'a, Message> {
        let label = if updating {
            "cache-settings-update"
        } else {
            "cache-settings-connect"
        };
        column![
            note(i18n, "cache-settings-config-details", colors::TEXT_MUTED),
            row![
                note(i18n, "cache-settings-config-changes", colors::TEXT_MUTED).width(Fill),
                button(text(i18n.text(label)))
                    .padding([7, 12])
                    .style(buttons::primary)
                    .on_press_maybe(can_connect.then_some(Message::Connect)),
            ]
            .align_y(Center)
            .spacing(spacing::LG),
        ]
        .push(self.connection_status(i18n))
        .spacing(spacing::MD)
    }

    fn forget_section<'a>(&self, i18n: &I18n, has_settings: bool) -> Column<'a, Message> {
        let idle = !self.busy();
        let guide = rich_text([span(i18n.text("cache-settings-guide"))
            .color(if idle {
                colors::PRIMARY
            } else {
                colors::TEXT_MUTED
            })
            .underline(true)
            .link_maybe(idle.then_some(()))])
        .size(14)
        .width(Fill)
        .on_link_click(|()| Message::OpenGuide);
        let guide_error = self.guide_failed.then(|| {
            let mut args = FluentArgs::new();
            args.set("url", GUIDE_URL);
            text(i18n.with_args("cache-settings-guide-error", &args))
                .size(13)
                .color(colors::WARNING)
        });
        let forget_label = i18n.text("cache-settings-forget");
        column![
            text(forget_label.clone()).size(17),
            note(i18n, "cache-settings-forget-hint", colors::TEXT_MUTED),
            note(i18n, "cache-settings-files", colors::TEXT_MUTED),
            row![
                guide,
                button(text(forget_label).size(14))
                    .padding([7, 12])
                    .style(buttons::danger)
                    .on_press_maybe((idle && has_settings).then_some(Message::Forget)),
            ]
            .align_y(Center)
            .spacing(spacing::LG),
        ]
        .push(guide_error)
        .push(self.forget_status(i18n))
        .spacing(spacing::MD)
    }

    fn form<'a>(&'a self, i18n: &I18n, saved: Option<&Metadata>) -> (Column<'a, Message>, bool) {
        let editing = !self.busy();
        let valid_url = attic::server_url(&self.url);
        let valid_name = attic::valid_name(self.server_name.trim());
        let token = self
            .token_info
            .as_ref()
            .and_then(|result| result.as_ref().ok());
        let caches = token.map_or(&[][..], |token| token.cache_names.as_slice());
        let selected = self
            .cache_name
            .as_ref()
            .filter(|name| caches.contains(name));
        let can_connect = editing && valid_url.is_some() && valid_name && selected.is_some();
        let submit = can_connect.then_some(Message::Connect);
        let url = text_input("https://cache.example.org", &self.url)
            .on_input_maybe(editing.then_some(Message::UrlChanged))
            .on_submit_maybe(submit.clone())
            .size(15)
            .padding([8, 10])
            .style(inputs::input);
        let url_hint = if valid_url.as_ref().is_some_and(|url| url.scheme() == "http") {
            Some(note(i18n, "cache-settings-http", colors::WARNING))
        } else if valid_url.is_none() && !self.url.trim().is_empty() {
            Some(note(i18n, "cache-settings-error-url", colors::DANGER))
        } else {
            None
        };
        let name = text_input(attic::DEFAULT_SERVER_NAME, &self.server_name)
            .on_input_maybe(editing.then_some(Message::ServerNameChanged))
            .on_submit_maybe(submit.clone())
            .size(15)
            .padding([8, 10])
            .style(inputs::input);
        let token_placeholder = if saved.is_some() { "***" } else { "" };
        let token_input = text_input(token_placeholder, &self.token)
            .secure(true)
            .on_input_maybe(editing.then_some(Message::TokenChanged))
            .on_submit_maybe(submit)
            .size(15)
            .padding([8, 10])
            .style(inputs::input);

        let cache_input: Element<'_, Message> = if editing && !caches.is_empty() {
            pick_list(caches, selected, Message::CacheSelected)
                .placeholder(i18n.text("cache-settings-select-cache"))
                .width(Fill)
                .text_size(15)
                .padding([8, 10])
                .style(pick_lists::field)
                .menu_style(pick_lists::menu)
                .into()
        } else {
            let displayed = if editing {
                saved
                    .filter(|_| self.token.trim().is_empty())
                    .map(|saved| &saved.cache_name)
            } else {
                self.cache_name.as_ref()
            };
            text_input(
                &i18n.text("cache-settings-select-cache"),
                displayed.map_or("", String::as_str),
            )
            .size(15)
            .padding([8, 10])
            .style(inputs::input)
            .into()
        };

        let fields = column![
            column![text(i18n.text("cache-settings-url")).size(14), url]
                .push(url_hint)
                .spacing(spacing::SM),
            column![
                text(i18n.text("cache-settings-server-name")).size(14),
                name,
                note(i18n, "cache-settings-server-hint", colors::TEXT_MUTED),
            ]
            .push((!valid_name).then(|| note(i18n, "cache-settings-error-server", colors::DANGER)))
            .spacing(spacing::SM),
            column![
                text(i18n.text("cache-settings-token")).size(14),
                token_input,
                note(i18n, "cache-settings-token-hint", colors::TEXT_MUTED),
            ]
            .push(
                self.token_info
                    .as_ref()
                    .and_then(|result| result.as_ref().err())
                    .map(|&error| { note(i18n, token_error_key(error), colors::DANGER) })
            )
            .spacing(spacing::SM),
            column![
                text(i18n.text("cache-settings-cache-name")).size(14),
                cache_input
            ]
            .push(
                token
                    .filter(|token| token.has_patterns)
                    .map(|_| { note(i18n, "cache-settings-patterns", colors::WARNING) })
            )
            .push(
                token
                    .filter(|token| token.cache_names.is_empty())
                    .map(|_| { note(i18n, "cache-settings-no-caches", colors::DANGER) })
            )
            .spacing(spacing::SM),
        ]
        .spacing(spacing::LG);
        (fields, can_connect)
    }

    fn forget_status<'a>(&self, i18n: &I18n) -> Option<Text<'a>> {
        let (key, color) = match self.status {
            Status::Forgetting => ("cache-settings-forgetting", colors::INFO),
            Status::Forgotten => ("cache-settings-forgotten", colors::SUCCESS),
            Status::ForgetFailed => ("cache-settings-error-forget", colors::DANGER),
            _ => return None,
        };
        Some(note(i18n, key, color))
    }

    fn connection_status<'a>(&self, i18n: &I18n) -> Option<Column<'a, Message>> {
        let (key, color) = match self.status {
            Status::Connecting => ("cache-settings-connecting", colors::INFO),
            Status::Connected => ("cache-settings-connected", colors::SUCCESS),
            Status::Invalid(ValidationError::CacheName) => {
                ("cache-settings-error-cache", colors::DANGER)
            }
            Status::ConnectFailed(error) => {
                let key = match error.kind {
                    FailureKind::Launch => "cache-settings-error-launch",
                    FailureKind::Timeout => "cache-settings-error-timeout",
                    FailureKind::AccessDenied => "cache-settings-error-access",
                    FailureKind::ConfigWrite => "cache-settings-error-config",
                    FailureKind::MetadataWrite => "cache-settings-error-metadata",
                    FailureKind::VerificationSetup => "cache-settings-error-verify-dir",
                    FailureKind::Cli => match error.stage {
                        Stage::VerifyLogin | Stage::ApplyLogin => "cache-settings-error-login",
                        Stage::VerifyCache => "cache-settings-error-info",
                        _ => "cache-settings-error-use",
                    },
                };
                return Some(
                    column![note(i18n, key, colors::DANGER)]
                        .push(
                            error
                                .may_have_applied()
                                .then(|| note(i18n, "cache-settings-partial", colors::WARNING)),
                        )
                        .spacing(spacing::SM),
                );
            }
            _ => return None,
        };
        Some(column![note(i18n, key, color)])
    }
}

fn note<'a>(i18n: &I18n, key: &str, color: Color) -> Text<'a> {
    text(i18n.text(key)).size(13).color(color)
}

fn token_error_key(error: TokenError) -> &'static str {
    match error {
        TokenError::Malformed => "cache-settings-error-token",
        TokenError::Expiration => "cache-settings-error-expiration",
        TokenError::Expired => "cache-settings-error-expired",
        TokenError::NotYetValid => "cache-settings-error-not-yet-valid",
        TokenError::Claims => "cache-settings-error-claims",
    }
}
