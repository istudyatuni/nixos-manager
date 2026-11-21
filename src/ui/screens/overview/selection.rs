use iced::{
    Color, Element, Font,
    widget::{
        text,
        text_editor::{self, Action, Binding, Content, Cursor, Position},
    },
};

use super::{DownloadStatus, Message, download_status_parts, journal};
use crate::{i18n::I18n, rebuild::Run, ui::style::inputs};

const LOG_CAPACITY: usize = 10_000;

#[derive(Clone, Copy)]
pub enum Target {
    Journal,
    RebuildError,
    DownloadError,
    DownloadDetail,
}

pub struct State {
    pub journal_expanded: bool,
    pub journal: Content,
    journal_lines: VecDeque<String>,
    pub rebuild_error: Content,
    pub download_error: Content,
    pub download_detail: Content,
}

impl Default for State {
    fn default() -> Self {
        Self {
            journal_expanded: true,
            journal: Content::new(),
            journal_lines: VecDeque::new(),
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
            Target::Journal => &mut self.journal,
            Target::RebuildError => &mut self.rebuild_error,
            Target::DownloadError => &mut self.download_error,
            Target::DownloadDetail => &mut self.download_detail,
        };
        content.perform(action);
    }

    pub fn has_journal(&self) -> bool {
        !self.journal_lines.is_empty()
    }

    pub fn clear_journal(&mut self) {
        self.journal_lines.clear();
        self.journal = Content::new();
    }

    pub fn append_journal(&mut self, lines: impl IntoIterator<Item = String>) {
        let mut lines = lines.into_iter().peekable();
        if lines.peek().is_none() {
            return;
        }
        self.journal_lines.extend(lines);
        let old_lines = self.journal_lines.len().saturating_sub(LOG_CAPACITY);
        let removed_bytes: usize = self
            .journal_lines
            .drain(..old_lines)
            .map(|line| line.len() + 1)
            .sum();
        let next = Content::with_text(&self.journal_lines.make_contiguous().join("\n"));
        let cursor = self.journal.cursor();
        // Translate byte offsets so trimming the log keeps selections on the retained text.
        let translate = |position| {
            let offset = byte_offset(&self.journal, position).saturating_sub(removed_bytes);
            position_at(&next, offset)
        };
        let cursor = Cursor {
            position: translate(cursor.position),
            selection: cursor.selection.map(translate),
        };
        self.journal = next;
        self.journal.move_to(cursor);
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

fn byte_offset(content: &Content, position: Position) -> usize {
    content
        .lines()
        .take(position.line)
        .map(|line| line.text.len() + line.ending.as_str().len())
        .sum::<usize>()
        + position.column
}

fn position_at(content: &Content, mut offset: usize) -> Position {
    let mut position = Position { line: 0, column: 0 };
    for (index, line) in content.lines().enumerate() {
        let length = line.text.len();
        position = Position {
            line: index,
            column: offset.min(length),
        };
        let next_line = length + line.ending.as_str().len();
        if offset <= length || offset < next_line {
            break;
        }
        offset -= next_line;
    }
    position
}
use std::collections::VecDeque;
