use iced::{
    Element, Event, Fill, Length, Rectangle, Renderer, Shrink, Size, Theme, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{Operation, Tree, tree},
    },
    widget::{Stack, container, mouse_area, opaque, responsive, space, stack},
};

use super::style::{containers, spacing};

const DIALOG_WIDTH: f32 = 560.0;

pub fn over<'a, Message: 'a>(
    background: Element<'a, Message>,
    dialog: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let layers = Stack::new().width(Fill).height(Fill);
    match dialog {
        Some(dialog) => layers.push(Element::new(Inactive(background))).push(dialog),
        None => layers.push(background),
    }
    .into()
}

pub fn dialog<'a, Message: Clone + 'a>(
    content: impl Fn() -> Element<'a, Message> + 'a,
    dismiss: Option<Message>,
) -> Element<'a, Message> {
    responsive(move |size| {
        let width = DIALOG_WIDTH.min(size.width);
        let panel = container(Element::new(FitHeight(content())))
            .width(width)
            .padding(spacing::XL)
            .style(containers::card);
        let backdrop = mouse_area(
            container(space())
                .width(Fill)
                .height(Fill)
                .style(containers::modal_backdrop),
        );
        let backdrop = match &dismiss {
            Some(message) => backdrop.on_press(message.clone()),
            None => backdrop,
        };
        let centered_panel = container(opaque(panel))
            .center(Fill)
            .padding([spacing::XL, 0.0]);
        stack![backdrop, centered_panel].into()
    })
    .into()
}

struct FitHeight<'a, Message>(Element<'a, Message>);

impl<Message> Widget<Message, Theme, Renderer> for FitHeight<'_, Message> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(self.0.as_widget())]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.0));
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.0.as_widget().size().width, Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let content = self.0.as_widget_mut();
        let state = &mut tree.children[0];
        let available = limits.max();
        let natural_limits =
            layout::Limits::new(Size::ZERO, Size::new(available.width, f32::INFINITY))
                .height(Shrink);
        let natural = content.layout(state, renderer, &natural_limits);

        if natural.size().height <= available.height {
            natural
        } else {
            // A bounded flex layout reserves room for the header and footer before sizing the scrollable form.
            content.layout(state, renderer, &layout::Limits::new(Size::ZERO, available))
        }
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.0
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
        self.0.as_widget_mut().update(
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

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.0
            .as_widget()
            .mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
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
        self.0.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.0.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

// Keep the background's widget tree while the dialog owns input, focus, and overlays.
struct Inactive<'a, Message>(Element<'a, Message>);

impl<Message> Widget<Message, Theme, Renderer> for Inactive<'_, Message> {
    fn tag(&self) -> tree::Tag {
        self.0.as_widget().tag()
    }

    fn state(&self) -> tree::State {
        self.0.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.0.as_widget().children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.0.as_widget().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        self.0.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.0.as_widget_mut().layout(tree, renderer, limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.0.as_widget().draw(
            tree,
            renderer,
            theme,
            style,
            layout,
            mouse::Cursor::Unavailable,
            viewport,
        );
    }
}
