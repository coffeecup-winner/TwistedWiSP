use iced::{
    alignment,
    event::Status,
    mouse::{self, Button, Cursor, Interaction},
    widget::{
        canvas::{Event, Frame, Geometry, Program, Text},
        Canvas,
    },
    Length, Point, Rectangle, Renderer, Size, Theme, Vector,
};

use twisted_wisp::{FlowNodeExtraData, WispContext};

#[derive(Debug, Clone, Copy)]
pub enum Message {}

#[derive(Debug)]
pub struct FlowGraphView {
    #[allow(dead_code)]
    flow_name: Option<String>,
    nodes: Vec<FlowGraphNodeView>,
}

#[derive(Debug)]
struct FlowGraphNodeView {
    pos: Point,
    size: Size,
    text: String,
}

impl FlowGraphView {
    pub fn new(flow_name: Option<String>, ctx: &WispContext) -> Self {
        let mut nodes = vec![];
        if let Some(flow) = flow_name
            .as_ref()
            .and_then(|name| ctx.get_function(name))
            .and_then(|f| f.as_flow())
        {
            for node_idx in flow.node_indices() {
                let node = flow.get_node(node_idx).unwrap();
                nodes.push(FlowGraphNodeView {
                    pos: Point::new(
                        node.extra_data
                            .get("x")
                            .unwrap_or(&FlowNodeExtraData::Integer(0))
                            .as_integer()
                            .unwrap() as f32,
                        node.extra_data
                            .get("y")
                            .unwrap_or(&FlowNodeExtraData::Integer(0))
                            .as_integer()
                            .unwrap() as f32,
                    ),
                    size: Size::new(
                        node.extra_data
                            .get("w")
                            .unwrap_or(&FlowNodeExtraData::Integer(80))
                            .as_integer()
                            .unwrap() as f32,
                        node.extra_data
                            .get("h")
                            .unwrap_or(&FlowNodeExtraData::Integer(40))
                            .as_integer()
                            .unwrap() as f32,
                    ),
                    text: node.display_text.clone(),
                });
            }
        }

        Self { flow_name, nodes }
    }

    pub fn view(&self) -> iced::Element<Message> {
        Canvas::new(self)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

#[derive(Debug, Default)]
pub struct FlowGraphViewState {
    pan_start: Option<Point>,
    viewport_offset: Vector,
}

impl Program<Message> for FlowGraphView {
    type State = FlowGraphViewState;

    fn update(
        &self,
        state: &mut Self::State,
        event: Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> (Status, Option<Message>) {
        #[allow(clippy::single_match)]
        match event {
            Event::Mouse(mouse_event) => match mouse_event {
                mouse::Event::ButtonPressed(Button::Middle) => {
                    if let Some(pos) = cursor.position() {
                        if bounds.contains(pos) {
                            state.pan_start = Some(pos - state.viewport_offset);
                            return (Status::Captured, None);
                        }
                    }
                }
                mouse::Event::ButtonReleased(Button::Middle) => {
                    state.pan_start = None;
                    return (Status::Captured, None);
                }
                mouse::Event::CursorMoved { position, .. } => {
                    if let Some(start) = state.pan_start {
                        state.viewport_offset = position - start;
                        return (Status::Captured, None);
                    }
                }
                _ => {}
            },
            _ => {}
        }
        (Status::Ignored, None)
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());

        for node in &self.nodes {
            frame.fill_rectangle(
                node.pos + state.viewport_offset,
                node.size,
                iced::Color::BLACK,
            );
            frame.fill_rectangle(
                node.pos + state.viewport_offset + Vector::new(1.0, 1.0),
                node.size - Size::new(2.0, 2.0),
                iced::Color::WHITE,
            );
            let text = Text {
                content: node.text.clone(),
                position: node.pos + state.viewport_offset + Vector::new(5.0, 5.0),
                size: 20.0.into(),
                color: iced::Color::BLACK,
                horizontal_alignment: alignment::Horizontal::Left,
                vertical_alignment: alignment::Vertical::Top,
                ..Default::default()
            };
            frame.fill_text(text);
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        _bounds: Rectangle,
        _cursor: Cursor,
    ) -> Interaction {
        if state.pan_start.is_some() {
            Interaction::Grab
        } else {
            Interaction::default()
        }
    }
}
