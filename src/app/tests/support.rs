use iced::{
    Element, Font, Pixels, Rectangle, Renderer, Size, Vector,
    advanced::{
        Layout,
        graphics::text::font_system,
        layout,
        renderer::Headless,
        widget::{Id, Operation, Tree, operation::Scrollable},
    },
};

use crate::{
    app::{App, Message},
    attic,
    i18n::LanguagePreference,
    rebuild,
    ui::icon,
};

#[derive(Default)]
pub(super) struct Labels {
    pub(super) text: Vec<(String, Rectangle)>,
    pub(super) containers: Vec<Rectangle>,
    pub(super) inputs: Vec<Rectangle>,
    pub(super) focusables: Vec<Rectangle>,
    pub(super) scroll_viewport: Option<(Rectangle, Vector)>,
}

impl Operation for Labels {
    fn container(&mut self, _id: Option<&Id>, bounds: Rectangle) {
        self.containers.push(bounds);
    }

    fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation)) {
        visit(self);
    }

    fn text(&mut self, _id: Option<&Id>, bounds: Rectangle, label: &str) {
        self.text.push((label.to_owned(), bounds));
    }

    fn text_input(
        &mut self,
        _id: Option<&Id>,
        bounds: Rectangle,
        _state: &mut dyn iced::advanced::widget::operation::TextInput,
    ) {
        self.inputs.push(bounds);
    }

    fn focusable(
        &mut self,
        _id: Option<&Id>,
        bounds: Rectangle,
        _state: &mut dyn iced::advanced::widget::operation::Focusable,
    ) {
        self.focusables.push(bounds);
    }

    fn scrollable(
        &mut self,
        _id: Option<&Id>,
        bounds: Rectangle,
        _content_bounds: Rectangle,
        translation: Vector,
        _state: &mut dyn Scrollable,
    ) {
        self.scroll_viewport = Some((bounds, translation));
    }
}

impl Labels {
    pub(super) fn bounds(&self, label: &str) -> Rectangle {
        self.text
            .iter()
            .find_map(|(text, bounds)| (text == label).then_some(*bounds))
            .unwrap_or_else(|| panic!("Missing label {label:?}"))
    }

    pub(super) fn contains(&self, fragment: &str) -> bool {
        self.text.iter().any(|(label, _)| label.contains(fragment))
    }
}

pub(super) fn layout(
    view: &mut Element<'_, Message>,
    renderer: &Renderer,
    size: Size,
) -> (Tree, layout::Node, Labels) {
    let widget = view.as_widget_mut();
    let mut tree = Tree::new(&*widget);
    let node = widget.layout(&mut tree, renderer, &layout::Limits::new(Size::ZERO, size));
    let mut labels = Labels::default();
    widget.operate(&mut tree, Layout::new(&node), renderer, &mut labels);
    (tree, node, labels)
}

pub(super) async fn headless() -> Renderer {
    font_system()
        .write()
        .expect("font system")
        .load_font(icon::FONT.into());
    <Renderer as Headless>::new(Font::DEFAULT, Pixels(16.0), Some("tiny-skia"))
        .await
        .expect("software renderer")
}

pub(super) fn boot_with_cache(cache: Result<Option<attic::Metadata>, attic::LoadError>) -> App {
    let (mut app, _) = App::boot();
    let _ = app.update(Message::Loaded(
        Ok(None),
        cache,
        false,
        rebuild::Tools::default(),
    ));
    app.set_language(LanguagePreference::English);
    app
}
