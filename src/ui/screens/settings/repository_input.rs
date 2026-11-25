use iced::{
    Element, Event, Length, Padding, Pixels, Rectangle, Renderer, Size, Theme, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{Operation, Tree, tree},
    },
    keyboard::{Event::KeyPressed, Key::Named, key::Named as Key},
    touch,
    widget::{overlay::menu, text_input},
};

use super::Message;
use crate::ui::{
    components::{
        icon,
        text_input::{State as InputState, TextInput},
    },
    style::pick_lists,
};

const MENU_HEIGHT: f32 = 200.0;
const MENU_LINE_HEIGHT: Pixels = Pixels(19.5);
const MENU_PADDING: Padding = Padding {
    top: 8.0,
    right: 10.0,
    bottom: 8.0,
    left: 10.0,
};
pub(super) struct RepositoryInput<'a> {
    input: TextInput<'a, Message>,
    on_focus: bool,
    suggestions: Option<Suggestions<'a>>,
    menu_style: menu::StyleFn<'a, Theme>,
}

struct Suggestions<'a> {
    value: &'a str,
    auto_branch: bool,
    filtered: Vec<&'a str>,
    unfiltered: Vec<&'a str>,
}

#[derive(Default)]
struct State {
    focused: bool,
    menu: menu::State,
    hovered: Option<usize>,
    mode: MenuMode,
    value: Option<String>,
}

#[derive(Default, PartialEq)]
enum MenuMode {
    #[default]
    Filtered,
    All,
    Closed,
}

impl State {
    fn show(&mut self, mode: MenuMode) {
        self.mode = mode;
        self.hovered = Some(0);
        self.menu = menu::State::default();
    }
}

impl<'a> RepositoryInput<'a> {
    pub(super) fn new(input: TextInput<'a, Message>, on_focus: bool) -> Self {
        Self {
            input,
            on_focus,
            suggestions: None,
            menu_style: Box::new(pick_lists::menu),
        }
    }

    pub(super) fn branches(
        mut self,
        value: &'a str,
        branches: &'a [String],
        auto_branch: bool,
    ) -> Self {
        self.input = self.input.icon(text_input::Icon {
            font: icon::TYPEFACE,
            code_point: icon::code_point("chevron_down"),
            size: Some(14.0.into()),
            spacing: 8.0,
            side: text_input::Side::Right,
        });
        self.suggestions = Some(Suggestions {
            value,
            auto_branch,
            filtered: filter_branches(branches, value),
            unfiltered: branches.iter().map(String::as_str).collect(),
        });
        self
    }

    pub(super) fn into_element(self) -> Element<'a, Message> {
        Element::new(self)
    }

    fn options(&self, state: &State) -> &[&str] {
        self.suggestions
            .as_ref()
            .map_or(&[], |suggestions| match state.mode {
                MenuMode::All => &suggestions.unfiltered,
                MenuMode::Filtered => &suggestions.filtered,
                MenuMode::Closed => &[],
            })
    }
}

fn filter_branches<'a>(branches: &'a [String], query: &str) -> Vec<&'a str> {
    let (mut prefixes, substrings): (Vec<_>, Vec<_>) = branches
        .iter()
        .map(String::as_str)
        .filter(|name| name.contains(query))
        .partition(|name| name.starts_with(query));
    prefixes.extend(substrings);
    prefixes
}

impl Widget<Message, Theme, Renderer> for RepositoryInput<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.input as &dyn Widget<_, _, _>)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.children[0].diff(&self.input as &dyn Widget<_, _, _>);
        let state = tree.state.downcast_mut::<State>();
        let value = self
            .suggestions
            .as_ref()
            .map(|suggestions| suggestions.value);
        if state.value.as_deref() != value {
            state.value = value.map(str::to_owned);
            // A default branch filled by the lookup should leave the picker open.
            if state.mode != MenuMode::All
                || !self
                    .suggestions
                    .as_ref()
                    .is_some_and(|items| items.auto_branch)
            {
                state.show(MenuMode::Filtered);
                // Enter submits a typed value until the user navigates the suggestions.
                state.hovered = None;
            }
        }
    }

    fn size(&self) -> Size<Length> {
        Widget::size(&self.input)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.input.layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.input
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
        let state = tree.state.downcast_mut::<State>();
        let input = &mut tree.children[0];
        let was_focused = state.focused;
        let pressed = matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                | Event::Touch(touch::Event::FingerPressed { .. })
        );
        let arrow_clicked = pressed && self.suggestions.is_some() && over_arrow(layout, cursor);
        let key = match event {
            Event::Keyboard(KeyPressed {
                key: Named(key),
                modifiers,
                ..
            }) if input.state.downcast_ref::<InputState>().is_focused() => Some((*key, *modifiers)),
            _ => None,
        };
        let open_from_keyboard = self.suggestions.is_some()
            && state.mode != MenuMode::All
            && matches!(key, Some((Key::ArrowDown, modifiers)) if modifiers.alt() || self.options(state).is_empty());
        let handled = if arrow_clicked || open_from_keyboard {
            input.state.downcast_mut::<InputState>().focus();
            if state.mode == MenuMode::All {
                state.show(MenuMode::Closed);
            } else {
                state.show(MenuMode::All);
            }
            true
        } else if let Some((key, _)) = key
            && (!self.options(state).is_empty()
                || key == Key::Escape && state.mode == MenuMode::All)
        {
            let options = self.options(state);
            match key {
                Key::ArrowDown => {
                    state.hovered = Some(
                        state
                            .hovered
                            .map_or(0, |index| (index + 1).min(options.len() - 1)),
                    );
                }
                Key::ArrowUp => {
                    state.hovered = Some(state.hovered.map_or(0, |index| index.saturating_sub(1)));
                }
                Key::Enter
                    if let Some(branch) = state.hovered.and_then(|index| options.get(index)) =>
                {
                    shell.publish(Message::BranchChanged((*branch).to_owned()));
                    input.state.downcast_mut::<InputState>().unfocus();
                }
                Key::Escape => state.show(MenuMode::Closed),
                _ => {}
            }
            matches!(key, Key::ArrowDown | Key::ArrowUp | Key::Escape)
                || matches!(key, Key::Enter) && state.hovered.is_some()
        } else {
            false
        };

        if handled {
            shell.capture_event();
            shell.request_redraw();
        } else if !matches!(key, Some((Key::Escape, _))) {
            // Let the dialog handle Escape when the branch picker is closed.
            self.input.update(
                input, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
        }
        let focused = input.state.downcast_ref::<InputState>().is_focused();
        if focused != was_focused {
            state.focused = focused;
            if focused && self.on_focus {
                shell.publish(Message::InputFocused);
            }
            if !focused {
                state.show(MenuMode::Filtered);
                state.hovered = None;
            }
            shell.request_redraw();
        }
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
        Widget::draw(
            &self.input,
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
        if self.suggestions.is_some() && over_arrow(layout, cursor) {
            mouse::Interaction::Pointer
        } else {
            self.input
                .mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'_>,
        _renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let input = tree.children[0].state.downcast_mut::<InputState>();
        let state = tree.state.downcast_mut::<State>();
        let options = self.options(state);
        let bounds = layout.bounds();
        if !input.is_focused() || options.is_empty() || !(bounds + translation).intersects(viewport)
        {
            return None;
        }
        let item_height = MENU_LINE_HEIGHT.0 + MENU_PADDING.y();
        let menu_height = (item_height * options.len() as f32).min(MENU_HEIGHT);
        Some(
            menu::Menu::new(
                &mut state.menu,
                options,
                &mut state.hovered,
                |branch: &str| {
                    input.unfocus();
                    Message::BranchChanged(branch.to_owned())
                },
                None,
                &self.menu_style,
            )
            .width(bounds.width)
            .padding(MENU_PADDING)
            .text_size(15)
            .text_line_height(MENU_LINE_HEIGHT)
            .overlay(
                layout.position() + translation,
                *viewport,
                bounds.height,
                menu_height.into(),
            ),
        )
    }
}

fn over_arrow(layout: Layout<'_>, cursor: mouse::Cursor) -> bool {
    cursor
        .position_over(layout.bounds())
        .is_some_and(|position| {
            layout
                .children()
                .nth(1)
                .is_some_and(|icon| position.x >= icon.bounds().x - 4.0)
        })
}
