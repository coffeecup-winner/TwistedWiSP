use std::collections::HashMap;

use iced::{
    alignment,
    event::Status,
    mouse::{self, Button, Cursor, Interaction},
    widget::{
        canvas::{path::Builder, Event, Frame, Geometry, Path, Program, Stroke, Text},
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
    connections: Vec<FlowGraphConnectionView>,
}

#[derive(Debug)]
struct FlowGraphNodeView {
    pos: Point,
    size: Size,
    text: String,
}

#[derive(Debug)]
struct FlowGraphConnectionView {
    from: usize,
    to: usize,
    output_index: u32,
    input_index: u32,
}

impl FlowGraphView {
    pub fn new(flow_name: Option<String>, ctx: &WispContext) -> Self {
        let mut nodes = vec![];
        let mut connections = vec![];

        if let Some(flow) = flow_name
            .as_ref()
            .and_then(|name| ctx.get_function(name))
            .and_then(|f| f.as_flow())
        {
            let mut node_idx_to_vector_idx = HashMap::new();
            for node_idx in flow.node_indices() {
                node_idx_to_vector_idx.insert(node_idx, nodes.len());

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
            for edge_idx in flow.edge_indices() {
                let (from, to, conn) = flow.get_connection(edge_idx).unwrap();
                connections.push(FlowGraphConnectionView {
                    from: node_idx_to_vector_idx[&from],
                    to: node_idx_to_vector_idx[&to],
                    output_index: conn.output_index,
                    input_index: conn.input_index,
                });
            }
        }

        Self {
            flow_name,
            nodes,
            connections,
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
                        state.viewport_offset.x = state.viewport_offset.x.round();
                        state.viewport_offset.y = state.viewport_offset.y.round();
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

        frame.translate(state.viewport_offset);

        for node in &self.nodes {
            frame.fill_rectangle(node.pos, node.size, iced::Color::BLACK);
            frame.fill_rectangle(
                node.pos + Vector::new(1.0, 1.0),
                node.size - Size::new(2.0, 2.0),
                iced::Color::WHITE,
            );
            let line = Path::line(
                node.pos + Vector::new(0.0, 30.0),
                node.pos + Vector::new(node.size.width, 30.0),
            );
            frame.stroke(&line, Stroke::default().with_color(iced::Color::BLACK));
            let text = Text {
                content: node.text.clone(),
                position: node.pos + Vector::new(5.0, 5.0),
                size: 20.0.into(),
                color: iced::Color::BLACK,
                horizontal_alignment: alignment::Horizontal::Left,
                vertical_alignment: alignment::Vertical::Top,
                ..Default::default()
            };
            frame.fill_text(text);
        }

        for conn in &self.connections {
            let from = &self.nodes[conn.from];
            let to = &self.nodes[conn.to];

            let start =
                from.pos + Vector::new(from.size.width, 50.0 + 30.0 * conn.output_index as f32);
            let end = to.pos + Vector::new(0.0, 50.0 + 30.0 * conn.input_index as f32);

            let line_x_size = (end.x - start.x).abs();
            let mut builder = Builder::new();
            builder.move_to(start);
            builder.bezier_curve_to(
                start + Vector::new(line_x_size * 0.4, 0.0),
                end - Vector::new(line_x_size * 0.4, 0.0),
                end,
            );

            let line = builder.build();
            frame.stroke(&line, Stroke::default().with_color(iced::Color::BLACK));
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
