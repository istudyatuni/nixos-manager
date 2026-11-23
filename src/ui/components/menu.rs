use iced::{
    Element, Event, Fill, Font, Length, Padding, Pixels, Rectangle, Renderer, Size, Theme, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget,
        graphics::text::Paragraph,
        layout, mouse, overlay, renderer,
        text::{self as paragraph, Paragraph as _, Shaping, Wrapping},
        widget::{Operation, Tree},
    },
    alignment, touch,
    widget::{Column, button, container, space, text},
};

use crate::ui::style::{buttons, containers, spacing};

const TEXT_SIZE: Pixels = Pixels(15.0);
const ITEM_PADDING: Padding = Padding {
    top: 7.0,
    right: spacing::LG,
    bottom: 7.0,
    left: spacing::LG,
};

pub fn panel<'a, Message: Clone + 'a, const N: usize>(
    options: [(String, Message); N],
) -> Element<'a, Message> {
    let label_width = options
        .iter()
        .map(|(label, _)| {
            Paragraph::with_text(paragraph::Text {
                content: label.as_str(),
                bounds: Size::INFINITE,
                size: TEXT_SIZE,
                line_height: paragraph::LineHeight::default(),
                font: Font::DEFAULT,
                align_x: paragraph::Alignment::Default,
                align_y: alignment::Vertical::Top,
                shaping: Shaping::Advanced,
                wrapping: Wrapping::None,
            })
            .min_width()
        })
        .fold(0.0, f32::max);
    let items = Column::with_children(options.into_iter().map(|(label, message)| {
        button(
            text(label)
                .size(TEXT_SIZE)
                .font(Font::DEFAULT)
                .shaping(Shaping::Advanced)
                .wrapping(Wrapping::WordOrGlyph),
        )
        .width(Fill)
        .padding(ITEM_PADDING)
        .style(buttons::menu_item)
        .on_press(message)
        .into()
    }))
    .width(label_width.ceil() + ITEM_PADDING.x());
    container(items)
        .padding(spacing::SM)
        .style(containers::card)
        .into()
}

pub fn anchored<'a, Message: Clone + 'a>(
    trigger: impl Into<Element<'a, Message>>,
    panel: Option<Element<'a, Message>>,
    dismiss: Message,
) -> Element<'a, Message> {
    Element::new(Anchored {
        trigger: trigger.into(),
        dismiss: panel.as_ref().map(|_| dismiss),
        panel: panel.unwrap_or_else(|| space().into()),
    })
}

struct Anchored<'a, Message> {
    trigger: Element<'a, Message>,
    panel: Element<'a, Message>,
    dismiss: Option<Message>,
}

impl<Message: Clone> Widget<Message, Theme, Renderer> for Anchored<'_, Message> {
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
        _viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
        Some(overlay::Element::new(Box::new(Popup {
            panel: self.panel.as_widget_mut(),
            tree: &mut tree.children[1],
            anchor: layout.bounds() + translation,
            dismiss: self.dismiss.as_ref()?,
        })))
    }
}

struct Popup<'a, Message> {
    panel: &'a mut dyn Widget<Message, Theme, Renderer>,
    tree: &'a mut Tree,
    anchor: Rectangle,
    dismiss: &'a Message,
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
        let position = iced::Point::new(
            self.anchor
                .x
                .clamp(0.0, (bounds.width - size.width).max(0.0)),
            y.clamp(0.0, (bounds.height - size.height).max(0.0)),
        );
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
        if !cursor.is_over(panel.bounds())
            && matches!(
                event,
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                    | Event::Touch(touch::Event::FingerPressed { .. })
            )
        {
            shell.publish(self.dismiss.clone());
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
        self.panel.mouse_interaction(
            self.tree,
            layout.children().next().unwrap(),
            cursor,
            &layout.bounds(),
            renderer,
        )
    }
}
