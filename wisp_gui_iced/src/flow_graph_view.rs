use std::collections::HashMap;

use iced::advanced::graphics::geometry::Renderer as GeometryRenderer;
use iced::advanced::layout::{self, Limits};
use iced::advanced::widget::Widget;
use iced::widget::button;
use iced::Theme;
use iced::{
    alignment,
    event::Status,
    mouse::{self, Button, Interaction},
    widget::canvas::{path::Builder, Frame, Path, Stroke, Text},
    Element, Length, Point, Rectangle, Renderer, Size, Vector,
};

use twisted_wisp::{FlowNodeExtraData, FlowNodeIndex, WispContext};

#[derive(Debug, Clone, Copy)]
pub enum Message {
    Dummy,
    MoveNodeTo(FlowNodeIndex, Point),
    ScrollTo(Vector),
}

const NODE_HEADER_HEIGHT: f32 = 30.0;
const NODE_CONNECTION_SLOT_OFFSET: f32 = 20.0;
const NODE_CONNECTION_SLOT_SPACING: f32 = 30.0;

pub struct FlowGraphView<'a, M> {
    #[allow(dead_code)]
    flow_name: Option<String>,
    nodes: Vec<FlowGraphNodeView<'a>>,
    connections: Vec<FlowGraphConnectionView>,
    size: Size,
    padding: Vector,
    f: Box<dyn Fn(Message) -> M>,
}

struct FlowGraphNodeView<'a> {
    id: FlowNodeIndex,
    pos: Point,
    size: Size,
    text: String,
    inputs: Vec<Point>,
    outputs: Vec<Point>,
    widget: Option<Element<'a, Message>>,
}

struct FlowGraphConnectionView {
    from: usize,
    to: usize,
    output_index: u32,
    input_index: u32,
}

impl<'a, M> FlowGraphView<'a, M> {
    pub fn new(
        flow_name: Option<String>,
        ctx: &WispContext,
        f: impl Fn(Message) -> M + 'static,
    ) -> Self {
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
                let func = ctx.get_function(&node.name).unwrap();

                let pos = Point::new(
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
                );

                let size = Size::new(
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
                );

                let mut inputs = vec![];
                let x = pos.x;
                let mut y = pos.y + NODE_HEADER_HEIGHT + NODE_CONNECTION_SLOT_OFFSET;
                for _ in 0..func.inputs().len() {
                    inputs.push(Point::new(x, y));
                    y += NODE_CONNECTION_SLOT_SPACING;
                }

                let mut outputs = vec![];
                let x = pos.x + size.width;
                let mut y = pos.y + NODE_HEADER_HEIGHT + NODE_CONNECTION_SLOT_OFFSET;
                for _ in 0..func.outputs().len() {
                    outputs.push(Point::new(x, y));
                    y += NODE_CONNECTION_SLOT_SPACING;
                }

                nodes.push(FlowGraphNodeView {
                    id: node_idx,
                    pos,
                    size,
                    text: node.display_text.clone(),
                    inputs,
                    outputs,
                    widget: Some(button("Load").on_press(Message::Dummy).into()),
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

        let mut view = Self {
            flow_name,
            nodes,
            connections,
            size: Size::ZERO,      // To be updated
            padding: Vector::ZERO, // To be updated
            f: Box::new(f),
        };
        view.update_size();
        view
    }

    fn get_node(&self, idx: usize) -> Option<&FlowGraphNodeView> {
        self.nodes.get(idx)
    }

    fn update_node_position(&mut self, node_idx: usize, new_pos: Point) {
        let node = self.nodes.get_mut(node_idx).unwrap();

        let offset = new_pos - node.pos;
        node.pos = new_pos;

        for input in node.inputs.iter_mut() {
            input.x += offset.x;
            input.y += offset.y;
        }

        for output in node.outputs.iter_mut() {
            output.x += offset.x;
            output.y += offset.y;
        }
    }

    fn update_size(&mut self) {
        const PADDING: f32 = 250.0;

        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for node in &self.nodes {
            min_x = min_x.min(node.pos.x);
            min_y = min_y.min(node.pos.y);
            max_x = max_x.max(node.pos.x + node.size.width);
            max_y = max_y.max(node.pos.y + node.size.height);
        }

        // Shift negative offsets
        min_x -= PADDING;
        min_y -= PADDING;
        for node in &mut self.nodes {
            node.pos.x -= min_x;
            node.pos.y -= min_y;
            for input in &mut node.inputs {
                input.x -= min_x;
                input.y -= min_y;
            }
            for output in &mut node.outputs {
                output.x -= min_x;
                output.y -= min_y;
            }
        }

        self.size = Size::new(max_x - min_x + 2.0 * PADDING, max_y - min_y + 2.0 * PADDING);
        self.padding = Vector::new(min_x, min_y);
    }

    fn hit_test_node_header(&self, pos: Point) -> Option<usize> {
        for (i, node) in self.nodes.iter().enumerate() {
            let header_rect = Rectangle {
                x: node.pos.x,
                y: node.pos.y,
                width: node.size.width,
                height: NODE_HEADER_HEIGHT,
            };
            if header_rect.contains(pos) {
                return Some(i);
            }
        }
        None
    }
}

#[derive(Debug, Default)]
struct ViewState {
    viewport_offset: Vector,
    pan_start: Option<Point>,
    grabbed_node: Option<GrabbedNode>,
}

#[derive(Debug, Default)]
struct GrabbedNode {
    node_idx: usize,
    offset: Vector,
}

impl<'a, M> Widget<M, Theme, Renderer> for FlowGraphView<'a, M> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &self,
        tree: &mut iced::advanced::widget::Tree,
        renderer: &Renderer,
        _limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        let mut children = vec![];
        let mut idx = 0;
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for node in &self.nodes {
            if let Some(widget) = &node.widget {
                let layout = widget.as_widget().layout(
                    &mut tree.children[idx],
                    renderer,
                    &Limits::new(Size::ZERO, node.size),
                );
                let layout = layout.move_to(node.pos);
                let bounds = layout.bounds();
                min_x = min_x.min(bounds.x);
                min_y = min_y.min(bounds.y);
                max_x = max_x.max(bounds.x + bounds.width);
                max_y = max_y.max(bounds.y + bounds.height);
                children.push(layout);
                idx += 1;
            }
        }

        let size = Size::new(max_x - min_x, max_y - min_y);
        let size = size.expand(Size::new(500.0, 500.0));

        layout::Node::with_children(size, children)
    }

    fn draw(
        &self,
        tree: &iced::advanced::widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &iced::advanced::renderer::Style,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        viewport: &Rectangle,
    ) {
        // TODO: Bounded drawing
        let mut frame = Frame::new(renderer, layout.bounds().size());

        // let state = tree.state.downcast_ref::<FlowGraphViewState>();
        // frame.translate(state.viewport_offset);

        for node in &self.nodes {
            frame.fill_rectangle(node.pos, node.size, iced::Color::BLACK);
            frame.fill_rectangle(
                node.pos + Vector::new(1.0, 1.0),
                node.size - Size::new(2.0, 2.0),
                iced::Color::WHITE,
            );
            let line = Path::line(
                node.pos + Vector::new(0.0, NODE_HEADER_HEIGHT),
                node.pos + Vector::new(node.size.width, NODE_HEADER_HEIGHT),
            );
            frame.stroke(&line, Stroke::default().with_color(iced::Color::BLACK));

            for input in &node.inputs {
                let path = Path::circle(*input, 5.0);
                frame.fill(&path, iced::Color::BLACK);
            }

            for output in &node.outputs {
                let path = Path::circle(*output, 5.0);
                frame.fill(&path, iced::Color::BLACK);
            }

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

            let start = from.outputs[conn.output_index as usize];
            let end = to.inputs[conn.input_index as usize];

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

        let geometry = vec![frame.into_geometry()];
        renderer.draw(geometry);

        let layouts = layout.children().collect::<Vec<_>>();

        let mut idx = 0;
        for node in &self.nodes {
            if let Some(widget) = &node.widget {
                widget.as_widget().draw(
                    &tree.children[idx],
                    renderer,
                    theme,
                    style,
                    layouts[idx],
                    cursor,
                    viewport,
                );
                idx += 1;
            }
        }
    }

    fn size_hint(&self) -> Size<Length> {
        self.size()
    }

    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        iced::advanced::widget::tree::Tag::of::<ViewState>()
    }

    fn state(&self) -> iced::advanced::widget::tree::State {
        iced::advanced::widget::tree::State::Some(Box::new(ViewState::default()))
    }

    fn children(&self) -> Vec<iced::advanced::widget::Tree> {
        let mut result = vec![];
        for node in &self.nodes {
            if let Some(widget) = &node.widget {
                result.push(iced::advanced::widget::Tree::new(widget));
            }
        }
        result
    }

    fn diff(&self, tree: &mut iced::advanced::widget::Tree) {
        let mut result = vec![];
        for node in &self.nodes {
            if let Some(widget) = &node.widget {
                result.push(widget);
            }
        }
        tree.diff_children(&result);
    }

    fn operate(
        &self,
        _state: &mut iced::advanced::widget::Tree,
        _layout: layout::Layout<'_>,
        _renderer: &Renderer,
        _operation: &mut dyn iced::advanced::widget::Operation<M>,
    ) {
    }

    fn on_event(
        &mut self,
        state: &mut iced::advanced::widget::Tree,
        event: iced::Event,
        _layout: layout::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut iced::advanced::Shell<'_, M>,
        _viewport: &Rectangle,
    ) -> iced::advanced::graphics::core::event::Status {
        #[allow(clippy::single_match)]
        match event {
            iced::Event::Mouse(mouse_event) => match mouse_event {
                mouse::Event::ButtonPressed(Button::Left) => {
                    let state = state.state.downcast_mut::<ViewState>();
                    if let Some(pos) = cursor.position() {
                        if let Some(node_idx) = self.hit_test_node_header(pos) {
                            state.grabbed_node = Some(GrabbedNode {
                                node_idx,
                                offset: pos - self.get_node(node_idx).unwrap().pos,
                            });
                            return Status::Captured;
                        }
                    }
                }
                mouse::Event::ButtonReleased(Button::Left) => {
                    let state = state.state.downcast_mut::<ViewState>();
                    if let Some(grabbed_node) = state.grabbed_node.take() {
                        let node = self.get_node(grabbed_node.node_idx).unwrap();
                        shell.publish((self.f)(Message::MoveNodeTo(
                            node.id,
                            node.pos + self.padding,
                        )));
                        return Status::Captured;
                    }
                }
                mouse::Event::CursorMoved { position, .. } => {
                    let state = state.state.downcast_mut::<ViewState>();
                    if let Some(GrabbedNode { node_idx, offset }) = state.grabbed_node {
                        self.update_node_position(
                            node_idx,
                            position - offset + state.viewport_offset,
                        );
                        if self.get_node(node_idx).unwrap().widget.is_some() {
                            shell.invalidate_layout();
                        }
                        return Status::Captured;
                    }
                }
                _ => {}
            },
            _ => {}
        }
        Status::Ignored
    }

    fn mouse_interaction(
        &self,
        state: &iced::advanced::widget::Tree,
        _layout: layout::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> iced::advanced::mouse::Interaction {
        let state = state.state.downcast_ref::<ViewState>();

        if state.pan_start.is_some() {
            Interaction::Grab
        } else if state.grabbed_node.is_some() {
            Interaction::Grabbing
        } else if let Some(pos) = cursor.position() {
            if self.hit_test_node_header(pos).is_some() {
                Interaction::Pointer
            } else {
                Interaction::default()
            }
        } else {
            Interaction::default()
        }
    }

    // fn overlay<'a>(
    //     &'a mut self,
    //     _state: &'a mut iced::advanced::widget::Tree,
    //     _layout: layout::Layout<'_>,
    //     _renderer: &R,
    //     _translation: Vector,
    // ) -> Option<iced::advanced::overlay::Element<'a, M, T, R>> {
    //     None
    // }
}

impl<'a, M> From<FlowGraphView<'a, M>> for Element<'a, M>
where
    M: 'a,
{
    fn from(graph_view: FlowGraphView<'a, M>) -> Self {
        Self::new(graph_view)
    }
}
