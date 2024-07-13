use iced::widget::{column, container};
use iced::{Element, Length, Sandbox, Settings};

pub fn main() -> iced::Result {
    TwistedWispGui::run(Settings::default())
}

struct TwistedWispGui;

#[derive(Debug, Clone, Copy)]
enum Message {}

impl Sandbox for TwistedWispGui {
    type Message = Message;

    fn new() -> Self {
        Self
    }

    fn title(&self) -> String {
        format!("TwistedWiSP {}", env!("CARGO_PKG_VERSION"))
    }

    fn update(&mut self, message: Message) {
        match message {}
    }

    fn view(&self) -> Element<Message> {
        container(column![])
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x()
            .center_y()
            .into()
    }
}
