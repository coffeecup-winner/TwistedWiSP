mod config;
mod flow_graph_view;

use config::TwistedWispConfig;
use iced::widget::{column, container};
use iced::{Application, Command, Element, Length, Settings};
use twisted_wisp::WispContext;
use twisted_wisp_protocol::WispRunnerClient;

#[derive(Debug, Clone, Copy)]
enum Message {
    FlowGraphViewMessage(flow_graph_view::Message),
}

struct TwistedWispGui {
    #[allow(dead_code)]
    config: TwistedWispConfig,
    #[allow(dead_code)]
    runner: Option<WispRunnerClient>,
    #[allow(dead_code)]
    ctx: Option<WispContext>,

    flow_graph_view: flow_graph_view::FlowGraphView,
}

impl Application for TwistedWispGui {
    type Executor = iced::executor::Default;
    type Theme = iced::theme::Theme;
    type Flags = TwistedWispConfig;
    type Message = Message;

    fn new(config: Self::Flags) -> (Self, Command<Self::Message>) {
        let mut runner = WispRunnerClient::init(
            &config.executable_path,
            Some(512),
            Some(48000),
            config.midi_in_port.as_deref(),
        );
        let sys_info = runner.get_system_info();

        let mut ctx = WispContext::new(sys_info.num_channels);
        ctx.add_builtin_functions();
        ctx.load_core_functions(&config.core_path)
            .expect("Failed to load core functions");

        for f in ctx.functions_iter() {
            runner.context_add_or_update_functions(f.get_ir_functions(&ctx));
        }

        (
            Self {
                config,
                runner: Some(runner),
                ctx: Some(ctx),
                flow_graph_view: flow_graph_view::FlowGraphView::new(),
            },
            Command::none(),
        )
    }

    fn title(&self) -> String {
        format!("TwistedWiSP {}", env!("CARGO_PKG_VERSION"))
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::FlowGraphViewMessage(_) => Command::none(),
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

pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app_path = std::env::current_exe()?;
    let config_path = app_path.with_file_name("wisp.toml");
    let config = TwistedWispConfig::load_from_file(&config_path)?;

    Ok(TwistedWispGui::run(Settings::with_flags(config))?)
}
