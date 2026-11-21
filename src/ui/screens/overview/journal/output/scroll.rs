use std::cell::RefCell;

use iced::{
    Element, Event, Fill, Length, Rectangle, Renderer, Shrink, Size, Theme, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, renderer,
        widget::{Id, Operation, Tree, operation, tree},
    },
    widget::{Scrollable, scrollable},
};

use super::{Log, Message};

#[derive(Default)]
pub(super) struct Position {
    previous: RefCell<Option<Snapshot>>,
}

struct Snapshot {
    node: layout::Node,
    log_top: f64,
    offset: f64,
}

impl Position {
    pub(super) fn view<'a>(
        &'a self,
        content: Element<'a, Message>,
        log: &'a RefCell<Log>,
    ) -> Element<'a, Message> {
        Element::new(View {
            scrollable: scrollable(content)
                .width(Fill)
                .height(Shrink)
                .anchor_bottom(),
            position: self,
            log,
        })
    }
}

struct View<'a> {
    scrollable: Scrollable<'a, Message, Theme, Renderer>,
    position: &'a Position,
    log: &'a RefCell<Log>,
}

impl Widget<Message, Theme, Renderer> for View<'_> {
    fn tag(&self) -> tree::Tag {
        self.scrollable.tag()
    }

    fn state(&self) -> tree::State {
        self.scrollable.state()
    }

    fn children(&self) -> Vec<Tree> {
        self.scrollable.children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.scrollable.diff(tree);
    }

    fn size(&self) -> Size<Length> {
        self.scrollable.size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let node = self.scrollable.layout(tree, renderer, limits);
        let log_top = self.log.borrow().top();
        let content = &node.children()[0];
        let mut previous = self.position.previous.borrow_mut();
        let (old_node, discarded_height) = match previous.as_ref() {
            Some(old) => {
                let old_content = &old.node.children()[0];
                if old.node.size() == node.size()
                    && old_content.size() == content.size()
                    && old.log_top == log_top
                {
                    return node;
                }
                let discarded_height = if old_content.size().width == content.size().width {
                    log_top - old.log_top
                } else {
                    0.0
                };
                (Some(&old.node), discarded_height)
            }
            None => (None, 0.0),
        };

        // Iced stores the distance from the end. Read the position in the old
        // viewport before setting that distance for the new layout.
        let mut restore = Restore {
            max_offset: f64::from((content.size().height - node.size().height).max(0.0)),
            discarded_height,
            previous_offset: previous.as_ref().map(|snapshot| snapshot.offset),
            offset: 0.0,
        };
        self.scrollable.operate(
            tree,
            Layout::new(old_node.unwrap_or(&node)),
            renderer,
            &mut restore,
        );
        *previous = Some(Snapshot {
            node: node.clone(),
            log_top,
            offset: restore.offset,
        });
        node
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.scrollable.operate(tree, layout, renderer, operation);
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.scrollable
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.scrollable.update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.scrollable
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }
}

struct Restore {
    max_offset: f64,
    discarded_height: f64,
    previous_offset: Option<f64>,
    offset: f64,
}

impl Operation for Restore {
    fn traverse(&mut self, _operate: &mut dyn FnMut(&mut dyn Operation)) {}

    fn scrollable(
        &mut self,
        _id: Option<&Id>,
        bounds: Rectangle,
        content_bounds: Rectangle,
        translation: Vector,
        state: &mut dyn operation::Scrollable,
    ) {
        // Preserve fractional pixels across repeated trims. Iced reports a
        // rounded translation, so reuse our offset while that pixel matches.
        let offset = self
            .previous_offset
            .filter(|offset| offset.round() == f64::from(translation.y))
            .unwrap_or_else(|| f64::from(translation.y));
        let at_end = content_bounds.height - bounds.height - translation.y <= 0.5;
        self.offset = if self.previous_offset.is_none() || at_end {
            self.max_offset
        } else {
            (offset - self.discarded_height).clamp(0.0, self.max_offset)
        };
        state.scroll_to(operation::scrollable::AbsoluteOffset {
            x: None,
            y: Some((self.max_offset - self.offset) as f32),
        });
    }
}
