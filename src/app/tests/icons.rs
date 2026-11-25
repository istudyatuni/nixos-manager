use iced::{
    Color, Element, Rectangle, Size, Theme,
    advanced::{
        Layout, mouse,
        renderer::{self, Headless, Renderer as _},
    },
    widget::container,
};

use crate::ui::icon;

use super::support::{headless, layout};

#[tokio::test]
async fn lucide_subset_renders_each_icon() {
    let mut renderer = headless().await;
    let size = Size::new(32.0, 32.0);
    let viewport = Rectangle::with_size(size);
    let mut rendered = std::collections::HashSet::new();

    for &(name, glyph) in icon::ALL_ICONS {
        let mut view = Element::from(
            container(icon::render(glyph).size(24).color(Color::WHITE)).center(size.width),
        );
        let (tree, node, _) = layout(&mut view, &renderer, size);
        renderer.reset(viewport);
        view.as_widget().draw(
            &tree,
            &mut renderer,
            &Theme::Dark,
            &renderer::Style::default(),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &viewport,
        );
        let pixels = renderer.screenshot(Size::new(32, 32), 1.0, Color::BLACK);
        assert!(
            pixels.chunks_exact(4).any(|pixel| pixel[0] > 0),
            "{name} is blank"
        );
        assert!(rendered.insert(pixels), "{name} renders as another glyph");
    }
}
