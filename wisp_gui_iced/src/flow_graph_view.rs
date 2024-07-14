use iced::{
    event::Status,
    mouse::{self, Button, Cursor, Interaction},
    widget::{
        canvas::{Event, Frame, Geometry, Program},
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
    view_model: FlowGraphViewModel,
}

#[derive(Debug, Default)]
struct FlowGraphViewModel {
    nodes: Vec<Rectangle>,
}

impl FlowGraphView {
    pub fn new(flow_name: Option<String>, ctx: &WispContext) -> Self {
        let view_model = if let Some(flow) = flow_name
            .as_ref()
            .and_then(|name| ctx.get_function(name))
            .and_then(|f| f.as_flow())
        {
            let mut nodes = vec![];
            for node_idx in flow.node_indices() {
                let node = flow.get_node(node_idx).unwrap();
                nodes.push(Rectangle {
                    x: node
                        .extra_data
                        .get("x")
                        .unwrap_or(&FlowNodeExtraData::Integer(0))
                        .as_integer()
                        .unwrap() as f32,
                    y: node
                        .extra_data
                        .get("y")
                        .unwrap_or(&FlowNodeExtraData::Integer(0))
                        .as_integer()
                        .unwrap() as f32,
                    width: node
                        .extra_data
                        .get("w")
                        .unwrap_or(&FlowNodeExtraData::Integer(80))
                        .as_integer()
                        .unwrap() as f32,
                    height: node
                        .extra_data
                        .get("h")
                        .unwrap_or(&FlowNodeExtraData::Integer(40))
                        .as_integer()
                        .unwrap() as f32,
                });
            }
            FlowGraphViewModel { nodes }
        } else {
            FlowGraphViewModel::default()
        };

        Self {
            flow_name,
            view_model,
        }
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

        for node in &self.view_model.nodes {
            frame.fill_rectangle(
                Point::new(node.x, node.y) + state.viewport_offset,
                Size::new(node.width, node.height),
                iced::Color::BLACK,
            );
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
