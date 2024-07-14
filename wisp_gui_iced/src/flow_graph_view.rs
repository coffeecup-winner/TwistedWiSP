use iced::{
    mouse::Cursor,
    widget::{
        canvas::{Frame, Geometry},
        Canvas,
    },
    Length, Point, Rectangle, Renderer, Size, Theme,
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

impl iced::widget::canvas::Program<Message> for FlowGraphView {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());

        for node in &self.view_model.nodes {
            frame.fill_rectangle(
                Point::new(node.x, node.y),
                Size::new(node.width, node.height),
                iced::Color::BLACK,
            );
        }

        vec![frame.into_geometry()]
    }
}
