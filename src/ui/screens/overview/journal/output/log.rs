use std::{collections::VecDeque, ops::Range};

use iced::{
    Font, Pixels, Point, Rectangle, Renderer, Size,
    advanced::{
        graphics::text::{Paragraph, cosmic_text::Cursor},
        renderer::{self, Renderer as _},
        text::{self, Paragraph as _, Renderer as _},
    },
    alignment,
};

use crate::ui::style::colors;

const LOG_CAPACITY: usize = 10_000;
const FONT_SIZE: Pixels = Pixels(13.0);
const LINE_HEIGHT: Pixels = Pixels(FONT_SIZE.0 * 1.3);

#[derive(Default)]
pub(super) struct Log {
    lines: VecDeque<Line>,
    first: usize,
    laid_out: usize,
    width: f32,
    bottom: f64,
    selection: Option<(Position, Position)>,
}

struct Line {
    text: String,
    paragraph: Option<Paragraph>,
    top: f64,
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
struct Position {
    line: usize,
    byte: usize,
}

impl Log {
    pub(super) fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub(super) fn text(&self) -> Option<String> {
        (!self.is_empty()).then(|| {
            self.lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
    }

    pub(super) fn append(&mut self, lines: impl IntoIterator<Item = String>) {
        self.lines.extend(lines.into_iter().map(|text| Line {
            text,
            paragraph: None,
            top: 0.0,
        }));
        let excess = self.lines.len().saturating_sub(LOG_CAPACITY);
        self.lines.drain(..excess);
        self.first += excess;
        self.laid_out = self.laid_out.saturating_sub(excess);
        if let Some((anchor, end)) = &mut self.selection {
            let first = Position {
                line: self.first,
                byte: 0,
            };
            *anchor = (*anchor).max(first);
            *end = (*end).max(first);
        }
    }

    pub(super) fn layout(&mut self, width: f32) -> f32 {
        if self.width != width {
            self.width = width;
            self.bottom = 0.0;
            self.laid_out = 0;
        }
        let bounds = Size::new(width, f32::INFINITY);
        for line in self.lines.range_mut(self.laid_out..) {
            let paragraph = match &mut line.paragraph {
                Some(paragraph) => {
                    paragraph.resize(bounds);
                    paragraph
                }
                None => line
                    .paragraph
                    .insert(Paragraph::with_text(log_text(line.text.as_str(), bounds))),
            };
            line.top = self.bottom;
            self.bottom += f64::from(paragraph.min_height().max(LINE_HEIGHT.0));
        }
        self.laid_out = self.lines.len();
        (self.bottom - self.top()) as f32
    }

    pub(super) fn select_all(&mut self) {
        if let Some(last) = self.lines.back() {
            self.selection = Some((
                Position {
                    line: self.first,
                    byte: 0,
                },
                Position {
                    line: self.first + self.lines.len() - 1,
                    byte: last.text.len(),
                },
            ));
        }
    }

    pub(super) fn select_at(
        &mut self,
        bounds: Rectangle,
        viewport: Rectangle,
        point: Point,
        extend: bool,
    ) {
        let Some(visible) = bounds.intersection(&viewport) else {
            return;
        };
        let y = point.y.clamp(visible.y, visible.y + visible.height) - bounds.y;
        let index = self.line_at(y);
        let line = &self.lines[index];
        let paragraph = line.paragraph.as_ref().expect("laid-out log");
        let local_y = y - (line.top - self.top()) as f32;
        let byte = paragraph
            .buffer()
            .hit((point.x - bounds.x).max(0.0), local_y)
            .map_or(line.text.len(), |cursor| cursor.index);
        let end = Position {
            line: self.first + index,
            byte: line.text.floor_char_boundary(byte),
        };
        let anchor = match self.selection {
            Some((anchor, _)) if extend => anchor,
            _ => end,
        };
        self.selection = Some((anchor, end));
    }

    pub(super) fn copy_selection(&self) -> Option<String> {
        let Range { start, end } = self.selection()?;
        let mut output = String::new();
        for index in start.line..=end.line {
            let line = &self.lines[index - self.first].text;
            let from = if index == start.line { start.byte } else { 0 };
            let to = if index == end.line {
                end.byte
            } else {
                line.len()
            };
            if index != start.line {
                output.push('\n');
            }
            output.push_str(&line[from..to]);
        }
        Some(output)
    }

    pub(super) fn draw(
        &self,
        renderer: &mut Renderer,
        bounds: Rectangle,
        viewport: Rectangle,
        focused: bool,
    ) {
        let Some(visible) = bounds.intersection(&viewport) else {
            return;
        };
        let start = self.line_at(visible.y - bounds.y);
        let bottom = visible.y + visible.height;
        let top = self.top();
        let selection = if focused { self.selection() } else { None };

        for (index, line) in self.lines.range(start..).enumerate() {
            let y = bounds.y + (line.top - top) as f32;
            if y >= bottom {
                break;
            }
            let paragraph = line.paragraph.as_ref().expect("laid-out log");
            let id = self.first + start + index;
            if let Some(Range { start, end }) = selection
                && (start.line..=end.line).contains(&id)
            {
                let from = Cursor::new(0, if id == start.line { start.byte } else { 0 });
                let to = Cursor::new(
                    0,
                    if id == end.line {
                        end.byte
                    } else {
                        line.text.len()
                    },
                );
                for run in paragraph.buffer().layout_runs() {
                    if let Some((x, width)) = run.highlight(from, to)
                        && let Some(bounds) = visible.intersection(&Rectangle {
                            x: bounds.x + x,
                            y: y + run.line_top,
                            width,
                            height: run.line_height,
                        })
                    {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds,
                                ..renderer::Quad::default()
                            },
                            colors::BORDER,
                        );
                    }
                }
            }
            if !line.text.is_empty() {
                let position = Point::new(bounds.x, y);
                let size = Size::new(bounds.width, paragraph.min_height());
                // Cached text compares the contents as well as the bounds
                // when the renderer checks what changed in a full journal.
                renderer.fill_text(
                    log_text(line.text.clone(), size),
                    position,
                    colors::TEXT,
                    Rectangle::new(position, size),
                );
            }
        }
    }

    fn selection(&self) -> Option<Range<Position>> {
        let (anchor, end) = self.selection?;
        (anchor != end).then(|| anchor.min(end)..anchor.max(end))
    }

    pub(super) fn top(&self) -> f64 {
        self.lines.front().map_or(self.bottom, |line| line.top)
    }

    fn line_at(&self, y: f32) -> usize {
        let y = self.top() + f64::from(y);
        self.lines
            .partition_point(|line| line.top <= y)
            .saturating_sub(1)
    }
}

fn log_text<T>(content: T, bounds: Size) -> text::Text<T> {
    text::Text {
        content,
        bounds,
        size: FONT_SIZE,
        line_height: text::LineHeight::Absolute(LINE_HEIGHT),
        font: Font::MONOSPACE,
        align_x: text::Alignment::Default,
        align_y: alignment::Vertical::Top,
        shaping: text::Shaping::Auto,
        wrapping: text::Wrapping::WordOrGlyph,
    }
}
