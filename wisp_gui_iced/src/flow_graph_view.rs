use iced::{
    mouse::Cursor,
    widget::{
        canvas::{Frame, Geometry},
        Canvas,
    },
    Length, Rectangle, Renderer, Theme,
};

#[derive(Debug, Clone, Copy)]
pub enum Message {}

#[derive(Debug, Default)]
pub struct FlowGraphView {}

impl FlowGraphView {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {}
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

        frame.fill_text("TEST");

        vec![frame.into_geometry()]
    }
}
