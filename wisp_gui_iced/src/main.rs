mod flow_graph_view;

use iced::widget::{column, container};
use iced::{Element, Length, Sandbox, Settings};

pub fn main() -> iced::Result {
    TwistedWispGui::run(Settings::default())
}

#[derive(Default)]
struct TwistedWispGui {
    flow_graph_view: flow_graph_view::FlowGraphView,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    FlowGraphViewMessage(flow_graph_view::Message),
}

impl Sandbox for TwistedWispGui {
    type Message = Message;

    fn new() -> Self {
        Self::default()
    }

    fn title(&self) -> String {
        format!("TwistedWiSP {}", env!("CARGO_PKG_VERSION"))
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::FlowGraphViewMessage(_) => {}
        }
    }

    fn view(&self) -> Element<Message> {
        let content = column![self
            .flow_graph_view
            .view()
            .map(Message::FlowGraphViewMessage)]
        .height(Length::Fill);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x()
            .center_y()
            .into()
    }
}
