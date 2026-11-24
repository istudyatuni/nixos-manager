use iced::{
    Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{Operation, Tree, tree},
    },
    border, touch,
    widget::{container, space, text},
    window,
};

use crate::ui::style::{colors, containers, spacing};

pub fn help<'a, Message: Clone + 'a>(description: String) -> Element<'a, Message> {
    const ICON_SIZE: f32 = 16.0;
    let icon = container(text("?").size(11).color(colors::INFO))
        .center(ICON_SIZE)
        .style(|_| {
            container::Style::default().border(
                border::rounded(ICON_SIZE / 2.0)
                    .width(1)
                    .color(colors::INFO),
            )
        });
    on_hover(
        icon,
        container(
            text(description)
                .size(13)
                .width(240)
                .wrapping(text::Wrapping::WordOrGlyph),
        )
        .padding(spacing::MD)
        .style(containers::card),
    )
}

/// The caller supplies the panel while it is open and handles the dismiss message.
pub fn on_click<'a, Message: Clone + 'a>(
    trigger: impl Into<Element<'a, Message>>,
    panel: Option<Element<'a, Message>>,
    dismiss: Message,
) -> Element<'a, Message> {
    Element::new(Anchored {
        trigger: trigger.into(),
        mode: Mode::Click(panel.as_ref().map(|_| dismiss)),
        panel: panel.unwrap_or_else(|| space().into()),
    })
}

/// Shows a passive panel that stays open as the pointer moves from the trigger onto it.
fn on_hover<'a, Message: Clone + 'a>(
    trigger: impl Into<Element<'a, Message>>,
    panel: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    Element::new(Anchored {
        trigger: trigger.into(),
        panel: panel.into(),
        mode: Mode::Hover,
    })
}

enum Mode<Message> {
    Click(Option<Message>),
    Hover,
}

#[derive(Default)]
struct State {
    hovering: bool,
}

struct Anchored<'a, Message> {
    trigger: Element<'a, Message>,
    panel: Element<'a, Message>,
    mode: Mode<Message>,
}

impl<Message: Clone> Widget<Message, Theme, Renderer> for Anchored<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.trigger), Tree::new(&self.panel)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.trigger, &self.panel]);
    }

    fn size(&self) -> Size<Length> {
        self.trigger.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        // Only the trigger takes up space in the parent layout.
        self.trigger
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.trigger
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
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
        // The trigger opens the panel. Once open, the overlay handles dismissal
        // because Iced can hide the cursor from widgets beneath the panel.
        if matches!(self.mode, Mode::Hover)
            && matches!(event, Event::Mouse(mouse::Event::CursorMoved { .. }))
            && layout
                .bounds()
                .intersection(viewport)
                .is_some_and(|bounds| cursor.is_over(bounds))
        {
            let state = tree.state.downcast_mut::<State>();
            if !state.hovering {
                state.hovering = true;
                shell.invalidate_layout();
                shell.request_redraw();
            }
        }
        self.trigger.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
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
        self.trigger.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
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
        if matches!(self.mode, Mode::Hover) && cursor.is_over(layout.bounds()) {
            return mouse::Interaction::Help;
        }
        self.trigger.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'a>,
        _renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
        // Apply the parent translation so the panel follows a scrolling trigger.
        let anchor = layout.bounds() + translation;
        let behavior = match &self.mode {
            Mode::Click(dismiss) => Behavior::Click(dismiss.as_ref()?),
            Mode::Hover => {
                let state = tree.state.downcast_mut::<State>();
                state.hovering &= anchor.intersection(viewport).is_some();
                if !state.hovering {
                    return None;
                }
                Behavior::Hover(state)
            }
        };
        Some(overlay::Element::new(Box::new(Popup {
            panel: self.panel.as_widget_mut(),
            tree: &mut tree.children[1],
            anchor,
            behavior,
        })))
    }
}

enum Behavior<'a, Message> {
    Click(&'a Message),
    Hover(&'a mut State),
}

struct Popup<'a, Message> {
    panel: &'a mut dyn Widget<Message, Theme, Renderer>,
    tree: &'a mut Tree,
    anchor: Rectangle,
    behavior: Behavior<'a, Message>,
}

impl<Message: Clone> overlay::Overlay<Message, Theme, Renderer> for Popup<'_, Message> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let panel = self.panel.layout(
            self.tree,
            renderer,
            &layout::Limits::new(Size::ZERO, bounds),
        );
        let size = panel.size();
        let below = self.anchor.y + self.anchor.height + spacing::SM;
        let y = if below + size.height <= bounds.height {
            below
        } else {
            self.anchor.y - spacing::SM - size.height
        };
        let x = match self.behavior {
            Behavior::Click(_) => self.anchor.x,
            Behavior::Hover(_) => self.anchor.center_x() - size.width / 2.0,
        };
        let position = iced::Point::new(
            x.clamp(0.0, (bounds.width - size.width).max(0.0)),
            y.clamp(0.0, (bounds.height - size.height).max(0.0)),
        );
        // The overlay uses window coordinates. The child node bounds the visible panel.
        layout::Node::with_children(bounds, vec![panel.move_to(position)])
    }

    fn operate(&mut self, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.panel.operate(
            self.tree,
            layout.children().next().unwrap(),
            renderer,
            operation,
        );
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let panel = layout.children().next().unwrap();
        let dismiss = match &mut self.behavior {
            Behavior::Click(dismiss) => *dismiss,
            Behavior::Hover(state) => {
                let bounds = panel.bounds();
                let keep_open = match event {
                    Event::Mouse(mouse::Event::CursorLeft)
                    | Event::Window(window::Event::Unfocused) => false,
                    Event::Mouse(_) | Event::Window(window::Event::RedrawRequested(_)) => {
                        over_hover_region(cursor, self.anchor, bounds)
                    }
                    _ => true,
                };
                if !keep_open {
                    state.hovering = false;
                    shell.invalidate_layout();
                    shell.request_redraw();
                }
                // A click on the tooltip should not reach controls behind it.
                if cursor.is_over(bounds)
                    && matches!(
                        event,
                        Event::Mouse(
                            mouse::Event::ButtonPressed(_) | mouse::Event::ButtonReleased(_)
                        )
                    )
                {
                    shell.capture_event();
                }
                return;
            }
        };
        if !cursor.is_over(panel.bounds())
            && matches!(
                event,
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                    | Event::Touch(touch::Event::FingerPressed { .. })
            )
        {
            shell.publish(dismiss.clone());
        } else {
            self.panel.update(
                self.tree,
                event,
                panel,
                cursor,
                renderer,
                clipboard,
                shell,
                &layout.bounds(),
            );
        }
        if matches!(event, Event::Mouse(_) | Event::Touch(_)) {
            shell.capture_event();
        }
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.panel.draw(
            self.tree,
            renderer,
            theme,
            style,
            layout.children().next().unwrap(),
            cursor,
            &layout.bounds(),
        );
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let panel = layout.children().next().unwrap();
        if matches!(self.behavior, Behavior::Hover(_)) && cursor.is_over(panel.bounds()) {
            return mouse::Interaction::Idle;
        }
        self.panel
            .mouse_interaction(self.tree, panel, cursor, &layout.bounds(), renderer)
    }
}

fn over_hover_region(cursor: mouse::Cursor, anchor: Rectangle, panel: Rectangle) -> bool {
    // Use the panel width across the gap so the pointer can approach it diagonally.
    let gap_top = (anchor.y + anchor.height).min(panel.y + panel.height);
    let gap = Rectangle {
        x: panel.x,
        y: gap_top,
        width: panel.width,
        height: (anchor.y.max(panel.y) - gap_top).max(0.0),
    };
    cursor.is_over(anchor) || cursor.is_over(panel) || cursor.is_over(gap)
}
