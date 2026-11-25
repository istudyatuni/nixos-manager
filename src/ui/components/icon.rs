use iced::{
    Background, Color, Element, Length, Rectangle, Renderer, Size, Theme,
    advanced::{Layout, Widget, layout, mouse, renderer, renderer::Renderer as _, widget::Tree},
    border,
};

const SIZE: f32 = 16.0;
const STROKE: f32 = 1.25;

pub enum Icon {
    Copy,
    Trash,
    Stop,
    ClearSelection,
}

impl<Message> Widget<Message, Theme, Renderer> for Icon {
    fn size(&self) -> Size<Length> {
        Size::new(SIZE.into(), SIZE.into())
    }

    fn layout(&mut self, _: &mut Tree, _: &Renderer, limits: &layout::Limits) -> layout::Node {
        layout::atomic(limits, SIZE, SIZE)
    }

    fn draw(
        &self,
        _: &Tree,
        renderer: &mut Renderer,
        _: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        _: mouse::Cursor,
        _: &Rectangle,
    ) {
        let origin = layout.position();
        let color = style.text_color;
        let rect = |x, y, width, height| Rectangle {
            x: origin.x + x,
            y: origin.y + y,
            width,
            height,
        };
        let mut draw = |bounds, radius, outlined| {
            let (background, border) = if outlined {
                (
                    Color::TRANSPARENT,
                    border::rounded(radius).color(color).width(STROKE),
                )
            } else {
                (color, border::rounded(radius))
            };
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border,
                    ..renderer::Quad::default()
                },
                Background::Color(background),
            );
        };

        match self {
            Self::Copy => {
                // Show only the exposed edges of the rear sheet.
                draw(rect(2.0, 1.5, 9.0, STROKE), 0.0, false);
                draw(rect(2.0, 1.5, STROKE, 10.5), 0.0, false);
                draw(rect(2.0, 12.0 - STROKE, 3.0, STROKE), 0.0, false);
                draw(rect(5.0, 4.5, 9.0, 10.0), 1.5, true);
            }
            Self::Trash => {
                draw(rect(4.0, 5.0, 8.0, 9.5), 1.5, true);
                draw(rect(2.5, 4.0, 11.0, STROKE), 0.5, false);
                draw(rect(6.0, 2.0, 4.0, 2.5), 0.75, false);
            }
            Self::Stop => draw(rect(3.0, 3.0, 10.0, 10.0), 1.0, false),
            Self::ClearSelection => {
                draw(rect(2.5, 2.5, 11.0, 11.0), 2.0, true);
                draw(rect(5.0, (SIZE - STROKE) / 2.0, 6.0, STROKE), 0.5, false);
            }
        }
    }
}

impl<'a, Message: 'a> From<Icon> for Element<'a, Message> {
    fn from(icon: Icon) -> Self {
        Self::new(icon)
    }
}
