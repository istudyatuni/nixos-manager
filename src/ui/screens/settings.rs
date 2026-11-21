use std::{fmt, path::Path};

use fluent_bundle::FluentArgs;
use iced::{
    Center, Element, Fill,
    widget::{
        Column, Row, TextInput, button, column, container, pick_list, row, scrollable, space, text,
        text_input,
    },
};

use crate::{
    config::{CONFIG_DIRECTORY, Config, DEFAULT_REPO_PATH, SaveError, display_repo_path},
    i18n::{I18n, LanguagePreference},
    ui::style::{buttons, colors, inputs, pick_lists, spacing},
};

#[derive(Clone)]
pub enum Message {
    LanguageChanged(LanguagePreference),
    RepositoryChanged(String),
    TokenChanged(String),
    PathChanged(String),
    BranchChanged(String),
    Save,
}

pub struct State {
    draft: Config,
    error: Option<SaveError>,
    saving: bool,
}

impl State {
    pub fn new(config: Option<&Config>, home: Option<&Path>) -> Self {
        let mut draft = config.cloned().unwrap_or_default();
        draft.path = display_repo_path(&draft.path, home).into_owned();
        Self {
            draft,
            error: None,
            saving: false,
        }
    }

    pub fn saving(&self) -> bool {
        self.saving
    }

    pub fn save_failed(&mut self, error: SaveError) {
        self.saving = false;
        self.error = Some(error);
    }

    pub fn update(&mut self, message: Message, can_save: bool) -> Option<Config> {
        if self.saving || (!can_save && matches!(message, Message::Save)) {
            return None;
        }

        self.error = None;
        match message {
            Message::LanguageChanged(value) => self.draft.language = value,
            Message::RepositoryChanged(value) => self.draft.repository = value,
            Message::TokenChanged(value) => self.draft.token = value,
            Message::PathChanged(value) => self.draft.path = value.into(),
            Message::BranchChanged(value) => self.draft.branch = value,
            Message::Save => {
                self.saving = true;
                return Some(self.draft.clone());
            }
        }
        None
    }

    pub fn view<'a>(&'a self, i18n: &'a I18n, can_save: bool) -> Element<'a, Message> {
        let editing = !self.saving;
        let repository = text_input(
            &i18n.text("settings-repository-placeholder"),
            &self.draft.repository,
        )
        .on_input_maybe(editing.then_some(Message::RepositoryChanged));
        let token = text_input("", &self.draft.token)
            .secure(true)
            .on_input_maybe(editing.then_some(Message::TokenChanged));
        let mut path_args = FluentArgs::new();
        path_args.set("path", DEFAULT_REPO_PATH);
        let path = text_input(
            &i18n.with_args("settings-path-placeholder", &path_args),
            &self.draft.path.to_string_lossy(),
        )
        .on_input_maybe(editing.then_some(Message::PathChanged));
        let branch = text_input(
            &i18n.text("settings-branch-placeholder"),
            &self.draft.branch,
        )
        .on_input_maybe(editing.then_some(Message::BranchChanged))
        .on_submit_maybe((editing && can_save).then_some(Message::Save));

        let mut form = column![
            language_field(i18n, self.draft.language, editing),
            field(i18n.text("settings-repository"), repository),
            field(i18n.text("settings-token"), token),
            field(i18n.text("settings-path"), path).push(
                text(i18n.text("settings-path-hint"))
                    .size(13)
                    .color(colors::TEXT_MUTED)
            ),
            field(i18n.text("settings-branch"), branch),
        ]
        .spacing(spacing::XL);

        if let Some(error) = self.error {
            form = form.push(text(error_text(i18n, error)).size(14).color(colors::DANGER));
        }
        if !can_save {
            form = form.push(
                text(i18n.text("settings-operation-in-progress"))
                    .size(14)
                    .color(colors::INFO),
            );
        }

        let save_label = if self.saving {
            "settings-saving"
        } else {
            "settings-save"
        };
        form = form.push(
            button(text(i18n.text(save_label)))
                .padding([7, 12])
                .style(buttons::primary)
                .on_press_maybe((editing && can_save).then_some(Message::Save)),
        );

        let form = container(form).width(Fill).max_width(488);
        scrollable(
            container(form)
                .padding(spacing::PAGE_PADDING)
                .center_x(Fill),
        )
        .height(Fill)
        .into()
    }
}

#[derive(Clone, PartialEq)]
struct LanguageOption {
    preference: LanguagePreference,
    label: String,
}

impl fmt::Display for LanguageOption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

fn language_field(i18n: &I18n, selected: LanguagePreference, editing: bool) -> Row<'_, Message> {
    let options = [
        (LanguagePreference::System, "settings-language-system"),
        (LanguagePreference::English, "settings-language-english"),
        (LanguagePreference::Russian, "settings-language-russian"),
    ]
    .map(|(preference, key)| LanguageOption {
        preference,
        label: i18n.text(key),
    });
    let selected = options
        .iter()
        .find(|option| option.preference == selected)
        .cloned();

    let width = 176;
    let control: Element<'_, Message> = if editing {
        pick_list(options, selected, |option| {
            Message::LanguageChanged(option.preference)
        })
        .width(width)
        .text_size(15)
        .padding([8, 10])
        .style(pick_lists::field)
        .menu_style(pick_lists::menu)
        .into()
    } else {
        text_input("", selected.as_ref().map_or("", |option| &option.label))
            .width(width)
            .size(15)
            .padding([8, 10])
            .style(inputs::input)
            .into()
    };

    row![
        text(i18n.text("settings-language")).size(14),
        space().width(Fill),
        control,
    ]
    .align_y(Center)
    .spacing(spacing::LG)
}

fn field(label: String, input: TextInput<'_, Message>) -> Column<'_, Message> {
    column![
        text(label).size(14),
        input.size(15).padding([8, 10]).style(inputs::input),
    ]
    .spacing(spacing::SM)
}

fn error_text(i18n: &I18n, error: SaveError) -> String {
    match error {
        SaveError::Repository => i18n.text("settings-error-repository"),
        SaveError::Token => i18n.text("settings-error-token"),
        SaveError::Path => i18n.text("settings-error-path"),
        SaveError::Branch => i18n.text("settings-error-branch"),
        SaveError::Write => {
            let mut args = FluentArgs::new();
            args.set("directory", format!("~/{CONFIG_DIRECTORY}"));
            i18n.with_args("settings-error-write", &args)
        }
    }
}
