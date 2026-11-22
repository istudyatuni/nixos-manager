use iced::{
    Color, Element, Font,
    widget::{
        text,
        text_editor::{self, Action, Binding, Content},
    },
};

use super::{DownloadStatus, Message, download_status_parts, journal};
use crate::{i18n::I18n, rebuild::Run, ui::style::inputs};

#[derive(Clone, Copy)]
pub enum Target {
    RebuildError,
    DownloadError,
    DownloadDetail,
}

pub struct State {
    pub journal_expanded: bool,
    pub(super) journal: journal::Output,
    pub rebuild_error: Content,
    pub download_error: Content,
    pub download_detail: Content,
}

impl Default for State {
    fn default() -> Self {
        Self {
            journal_expanded: true,
            journal: journal::Output::default(),
            rebuild_error: Content::new(),
            download_error: Content::new(),
            download_detail: Content::new(),
        }
    }
}

impl State {
    pub fn update_selection(&mut self, target: Target, action: Action) {
        if action.is_edit() {
            return;
        }

        let content = match target {
            Target::RebuildError => &mut self.rebuild_error,
            Target::DownloadError => &mut self.download_error,
            Target::DownloadDetail => &mut self.download_detail,
        };
        content.perform(action);
    }

    pub fn has_journal(&self) -> bool {
        !self.journal.is_empty()
    }

    pub fn journal_text(&self) -> Option<String> {
        self.journal.text()
    }

    pub fn clear_journal(&mut self) {
        self.journal = journal::Output::default();
    }

    pub fn append_journal(&mut self, lines: impl IntoIterator<Item = String>) {
        self.journal.append(lines);
    }

    pub fn set_rebuild_error(&mut self, i18n: &I18n, run: Option<&Run>) {
        self.rebuild_error = match run.filter(|run| journal::has_error(run)) {
            Some(run) => Content::with_text(&journal::rebuild_status(i18n, run).0),
            None => Content::new(),
        };
    }

    pub fn set_download_error(&mut self, i18n: &I18n, download: &DownloadStatus) {
        if matches!(download, DownloadStatus::Finished(Err(_)))
            && let Some((key, _, detail)) = download_status_parts(download)
        {
            self.download_error = Content::with_text(&i18n.text(key));
            self.download_detail = Content::with_text(detail.unwrap_or_default());
        } else {
            self.download_error = Content::new();
            self.download_detail = Content::new();
        }
    }
}

pub fn view(
    content: &Content,
    target: Target,
    font: Font,
    size: u32,
    color: Color,
) -> Element<'_, Message> {
    text_editor::TextEditor::new(content)
        .font(font)
        .size(size)
        .padding(0)
        .wrapping(text::Wrapping::WordOrGlyph)
        .style(move |_, _| inputs::selectable_text(color))
        .on_action(move |action| Message::SelectText(target, action))
        .key_binding(|keypress| {
            Binding::from_key_press(keypress).filter(|binding| {
                matches!(
                    binding,
                    Binding::Copy
                        | Binding::Move(_)
                        | Binding::Select(_)
                        | Binding::SelectAll
                        | Binding::Unfocus
                )
            })
        })
        .into()
}
