use std::collections::VecDeque;

use iced::{Event, keyboard, widget::text_input::Cursor};

const UNDO_LIMIT: usize = 100;

#[derive(Default)]
pub(super) struct History {
    pub(super) value: String,
    undo: VecDeque<Snapshot>,
    redo: VecDeque<Snapshot>,
}

struct Snapshot {
    value: String,
    cursor: Cursor,
}

pub(super) enum Action {
    Undo,
    Redo,
}

impl Action {
    pub(super) fn from_event(event: &Event) -> Option<Self> {
        let Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            physical_key,
            modifiers,
            ..
        }) = event
        else {
            return None;
        };
        // Leave Ctrl+Alt combinations to normal input handling.
        if !modifiers.control() || modifiers.alt() {
            return None;
        }
        match (
            key.to_latin(*physical_key)?.to_ascii_lowercase(),
            modifiers.shift(),
        ) {
            ('z', false) => Some(Self::Undo),
            ('z', true) | ('y', false) => Some(Self::Redo),
            _ => None,
        }
    }
}

impl History {
    pub(super) fn new(value: String) -> Self {
        Self {
            value,
            ..Self::default()
        }
    }

    pub(super) fn record(&mut self, value: &str, cursor: Cursor) {
        if self.value == value {
            return;
        }
        if self.undo.len() == UNDO_LIMIT {
            self.undo.pop_front();
        }
        self.undo.push_back(Snapshot {
            value: std::mem::replace(&mut self.value, value.to_owned()),
            cursor,
        });
        self.redo.clear();
    }

    pub(super) fn restore(&mut self, action: Action, cursor: Cursor) -> Option<Cursor> {
        let (from, to) = match action {
            Action::Undo => (&mut self.undo, &mut self.redo),
            Action::Redo => (&mut self.redo, &mut self.undo),
        };
        let snapshot = from.pop_back()?;
        to.push_back(Snapshot {
            value: std::mem::replace(&mut self.value, snapshot.value),
            cursor,
        });
        Some(snapshot.cursor)
    }
}
