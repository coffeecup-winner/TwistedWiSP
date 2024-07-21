use std::collections::HashMap;

use iced::advanced::graphics::geometry::Renderer as GeometryRenderer;
use iced::advanced::layout::{self, Limits};
use iced::advanced::widget::Widget;
use iced::widget::button;
use iced::{
    alignment,
    event::Status,
    mouse::{self, Button, Cursor, Interaction},
    widget::canvas::{path::Builder, Event, Frame, Geometry, Path, Program, Stroke, Text},
    Element, Length, Point, Rectangle, Renderer, Size, Theme, Vector,
};

use twisted_wisp::{FlowNodeExtraData, WispContext};

#[derive(Debug, Clone, Copy)]
pub enum Message {
    Dummy,
    ScrollTo(Vector),
}

const NODE_HEADER_HEIGHT: f32 = 30.0;
const NODE_CONNECTION_SLOT_OFFSET: f32 = 20.0;
const NODE_CONNECTION_SLOT_SPACING: f32 = 30.0;

pub struct FlowGraphView<'a, M, T> {
    #[allow(dead_code)]
    flow_name: Option<String>,
    nodes: Vec<FlowGraphNodeView<'a, T>>,
    connections: Vec<FlowGraphConnectionView>,
    size: Size,
    f: Box<dyn Fn(Message) -> M>,
}

struct FlowGraphNodeView<'a, T> {
    pos: Point,
    size: Size,
    text: String,
    inputs: Vec<Point>,
    outputs: Vec<Point>,
    widget: Option<Element<'a, Message, T>>,
}

struct FlowGraphConnectionView {
    from: usize,
    to: usize,
    output_index: u32,
    input_index: u32,
}

impl<'a, M, T> FlowGraphView<'a, M, T>
where
    T: iced::widget::button::StyleSheet
        + iced::widget::text::StyleSheet
        + iced::widget::container::StyleSheet
        + 'a,
{
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
            size: Size::new(0.0, 0.0), // To be updated
            f: Box::new(f),
        };
        view.update_size();
        view
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
    }
}

#[derive(Debug, Default)]
pub struct FlowGraphViewState {
    pan_start: Option<Point>,
    viewport_offset: Vector,
}

impl<'a, M, T> Widget<M, T, Renderer> for FlowGraphView<'a, M, T> {
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
        dbg!(size);

        layout::Node::with_children(size, children)
    }

    fn draw(
        &self,
        tree: &iced::advanced::widget::Tree,
        renderer: &mut Renderer,
        theme: &T,
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
        iced::advanced::widget::tree::Tag::of::<FlowGraphViewState>()
    }

    fn state(&self) -> iced::advanced::widget::tree::State {
        iced::advanced::widget::tree::State::Some(Box::new(FlowGraphViewState::default()))
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
        viewport: &Rectangle,
    ) -> iced::advanced::graphics::core::event::Status {
        // iced::advanced::graphics::core::event::Status::Ignored
        #[allow(clippy::single_match)]
        match event {
            iced::Event::Mouse(mouse_event) => match mouse_event {
                mouse::Event::ButtonPressed(Button::Middle) => {
                    let state = state.state.downcast_mut::<FlowGraphViewState>();
                    if let Some(pos) = cursor.position() {
                        if viewport.contains(pos) {
                            state.pan_start = Some(pos - state.viewport_offset);
                            return Status::Captured;
                        }
                    }
                }
                mouse::Event::ButtonReleased(Button::Middle) => {
                    let state = state.state.downcast_mut::<FlowGraphViewState>();
                    state.pan_start = None;
                    return Status::Captured;
                }
                mouse::Event::CursorMoved { position, .. } => {
                    let state = state.state.downcast_mut::<FlowGraphViewState>();
                    if let Some(start) = state.pan_start {
                        state.viewport_offset = position - start;
                        state.viewport_offset.x = state.viewport_offset.x.round();
                        state.viewport_offset.y = state.viewport_offset.y.round();
                        shell.publish((self.f)(Message::ScrollTo(
                            Vector::ZERO - state.viewport_offset,
                        )));
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
        _state: &iced::advanced::widget::Tree,
        _layout: layout::Layout<'_>,
        _cursor: iced::advanced::mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> iced::advanced::mouse::Interaction {
        iced::advanced::mouse::Interaction::Idle
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

impl<'a, M, T> From<FlowGraphView<'a, M, T>> for Element<'a, M, T>
where
    M: 'a,
    T: 'a,
{
    fn from(graph_view: FlowGraphView<'a, M, T>) -> Self {
        Self::new(graph_view)
    }
}

impl<'a, M, T> Program<Message> for FlowGraphView<'a, M, T> {
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
