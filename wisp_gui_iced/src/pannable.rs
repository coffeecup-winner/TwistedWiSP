use iced::{
    advanced::{
        layout::{self, Limits, Node},
        widget::{
            tree::{State, Tag},
            Tree,
        },
        Widget,
    },
    event::Status,
    mouse::{self, Button},
    Element, Length, Point, Renderer, Size, Theme,
};
use iced_widget::scrollable::Direction;

pub struct Pannable<'a, Message> {
    scrollable: Element<'a, Message>,
}

impl<'a, Message> Pannable<'a, Message>
where
    Message: 'a,
{
    pub fn new(scrollable: iced::widget::Scrollable<'a, Message>) -> Self {
        Self {
            scrollable: scrollable.into(),
        }
    }
}

pub fn pannable<'a, Message: 'a>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
) -> Pannable<'a, Message> {
    let scrollable =
        iced::widget::scrollable(content).direction(iced::widget::scrollable::Direction::Both {
            horizontal: Default::default(),
            vertical: Default::default(),
        });
    Pannable::new(scrollable)
}

struct ViewState {
    pan_start: Option<Point>,
}

impl<'a, Message> Widget<Message, Theme, Renderer> for Pannable<'a, Message>
where
    Message: 'a,
{
    fn size(&self) -> Size<Length> {
        self.scrollable.as_widget().size()
    }

    fn layout(&self, tree: &mut Tree, renderer: &Renderer, limits: &Limits) -> Node {
        // self.scrollable.as_widget().layout(tree, renderer, limits)
        layout::contained(limits, Length::Fill, Length::Fill, |limits| {
            self.scrollable
                .as_widget()
                .layout(&mut tree.children[0], renderer, limits)
        })
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &iced::advanced::renderer::Style,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        viewport: &iced::Rectangle,
    ) {
        self.scrollable.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout.children().next().unwrap(),
            cursor,
            viewport,
        )
    }

    fn size_hint(&self) -> Size<Length> {
        self.scrollable.as_widget().size_hint()
    }

    fn tag(&self) -> Tag {
        Tag::of::<ViewState>()
    }

    fn state(&self) -> State {
        State::Some(Box::new(ViewState { pan_start: None }))
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(Into::<&Element<_>>::into(&self.scrollable))]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.scrollable));
    }

    fn operate(
        &self,
        state: &mut Tree,
        layout: iced::advanced::Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation<Message>,
    ) {
        self.scrollable
            .as_widget()
            .operate(&mut state.children[0], layout, renderer, operation)
    }

    fn on_event(
        &mut self,
        state: &mut Tree,
        event: iced::Event,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut iced::advanced::Shell<'_, Message>,
        viewport: &iced::Rectangle,
    ) -> iced::advanced::graphics::core::event::Status {
        #[allow(clippy::single_match)]
        match event {
            iced::Event::Mouse(mouse_event) => match mouse_event {
                mouse::Event::ButtonPressed(Button::Middle) => {
                    let state = state.state.downcast_mut::<ViewState>();
                    if let Some(pos) = cursor.position() {
                        if viewport.contains(pos) {
                            state.pan_start = Some(pos);
                            return Status::Captured;
                        }
                    }
                }
                mouse::Event::ButtonReleased(Button::Middle) => {
                    let state = state.state.downcast_mut::<ViewState>();
                    if state.pan_start.is_some() {
                        state.pan_start = None;
                        return Status::Captured;
                    }
                }
                mouse::Event::CursorMoved { position, .. } => {
                    let view_state = state.state.downcast_mut::<ViewState>();
                    if let Some(start) = view_state.pan_start {
                        let delta = position - start;
                        view_state.pan_start = Some(position);
                        // Work with the scrollable state directly. It's exposed via the iced_widget crate,
                        // but not via the iced crate.
                        let scrollable_state = state.children[0]
                            .state
                            .downcast_mut::<iced_widget::scrollable::State>();
                        let scrollable_layout = layout.children().next().unwrap();
                        let scrollable_content_layout =
                            scrollable_layout.children().next().unwrap();
                        scrollable_state.scroll(
                            delta,
                            Direction::Both {
                                horizontal: Default::default(),
                                vertical: Default::default(),
                            },
                            scrollable_layout.bounds(),
                            scrollable_content_layout.bounds(),
                        );
                        return Status::Captured;
                    }
                }
                _ => {}
            },
            _ => {}
        }
        self.scrollable.as_widget_mut().on_event(
            &mut state.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        )
    }

    fn mouse_interaction(
        &self,
        state: &Tree,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        viewport: &iced::Rectangle,
        renderer: &Renderer,
    ) -> iced::advanced::mouse::Interaction {
        self.scrollable.as_widget().mouse_interaction(
            &state.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'b>(
        &'b mut self,
        state: &'b mut Tree,
        layout: iced::advanced::Layout<'_>,
        renderer: &Renderer,
        translation: iced::Vector,
    ) -> Option<iced::advanced::overlay::Element<'b, Message, Theme, Renderer>> {
        self.scrollable.as_widget_mut().overlay(
            &mut state.children[0],
            layout,
            renderer,
            translation,
        )
    }
}

impl<'a, Message> From<Pannable<'a, Message>> for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
{
    fn from(value: Pannable<'a, Message>) -> Self {
        Element::new(value)
    }
}
