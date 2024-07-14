mod config;
mod flow_graph_view;

use std::path::PathBuf;

use config::TwistedWispConfig;
use flow_graph_view::FlowGraphView;
use iced::widget::{button, column, container, toggler};
use iced::{Application, Command, Element, Length, Settings};
use twisted_wisp::{WispContext, WispFunction};
use twisted_wisp_ir::CallId;
use twisted_wisp_protocol::{DataIndex, WispRunnerClient};

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
enum Message {
    SetDspEnabled(bool),

    LoadFlowFromFile(String),

    FlowGraphViewMessage(flow_graph_view::Message),
}

struct TwistedWispGui {
    #[allow(dead_code)]
    config: TwistedWispConfig,
    #[allow(dead_code)]
    runner: WispRunnerClient,
    #[allow(dead_code)]
    ctx: WispContext,

    is_dsp_enabled: bool,

    flow_graph_view: FlowGraphView,
}

impl TwistedWispGui {
    fn config(&self) -> &TwistedWispConfig {
        &self.config
    }

    fn runner_mut(&mut self) -> &mut WispRunnerClient {
        &mut self.runner
    }

    fn ctx(&self) -> &WispContext {
        &self.ctx
    }

    fn ctx_mut(&mut self) -> &mut WispContext {
        &mut self.ctx
    }

    fn load_flow_from_file(&mut self, path: String) -> String {
        let flow_name = self
            .ctx_mut()
            .load_function(&PathBuf::from(path))
            .expect("Failed to load the flow function");
        let ctx = self.ctx();
        let flow = ctx.get_function(&flow_name).unwrap().as_flow().unwrap();
        let ir_functions = flow.get_ir_functions(ctx);
        let mut buffers = vec![];
        for (name, path) in flow.buffers() {
            let full_path = if let Some(path) = path {
                self.config()
                    .resolve_data_path(path)
                    .expect("Failed to resolve a data path")
                    .to_str()
                    .unwrap()
                    .to_owned()
            } else {
                // For built-in buffers
                "".to_owned()
            };
            buffers.push((name.clone(), full_path));
        }
        let mut buffer_nodes = vec![];
        for idx in flow.node_indices() {
            let node = flow.get_node(idx).unwrap();
            if let Some(buffer_name) = node.extra_data.get("buffer") {
                buffer_nodes.push((idx, buffer_name.as_string().unwrap().to_owned()));
            }
        }
        let mut value_nodes = vec![];
        for idx in flow.node_indices() {
            let node = flow.get_node(idx).unwrap();
            if let Some(value) = node.extra_data.get("value") {
                value_nodes.push((idx, value.as_float().unwrap()));
            }
        }
        let runner = self.runner_mut();
        runner.context_add_or_update_functions(ir_functions);
        for (name, path) in buffers {
            runner.context_load_wave_file(flow_name.clone(), name, path);
        }
        runner.context_set_main_function(flow_name.clone());
        runner.context_update();
        for (idx, buffer_name) in buffer_nodes {
            runner.context_set_data_array(
                flow_name.clone(),
                CallId(idx.index() as u32),
                DataIndex(0),
                buffer_name,
            );
        }
        for (idx, value) in value_nodes {
            runner.context_set_data_value(
                flow_name.clone(),
                CallId(idx.index() as u32),
                DataIndex(0),
                value,
            );
        }
        flow_name
    }

    fn set_dsp_enabled(&mut self, v: bool) {
        if v {
            self.runner_mut().dsp_start()
        } else {
            self.runner_mut().dsp_stop()
        }
        self.is_dsp_enabled = v;
    }
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

        let flow_graph_view = FlowGraphView::new(None, &ctx);
        (
            Self {
                config,
                runner,
                ctx,
                is_dsp_enabled: false,
                flow_graph_view,
            },
            Command::none(),
        )
    }

    fn title(&self) -> String {
        format!("TwistedWiSP {}", env!("CARGO_PKG_VERSION"))
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::SetDspEnabled(v) => {
                self.set_dsp_enabled(v);
                Command::none()
            }
            Message::LoadFlowFromFile(path) => {
                let flow_name = self.load_flow_from_file(path);
                self.flow_graph_view = FlowGraphView::new(Some(flow_name), self.ctx());
                Command::none()
            }
            Message::FlowGraphViewMessage(_) => Command::none(),
        }
    }

    fn view(&self) -> Element<Message> {
        // TODO: Fix this
        const PATH: &str = "wisp_gui/beat3.twf";

        let content = column![
            button("Load").on_press(Message::LoadFlowFromFile(PATH.to_owned())),
            toggler(Some("DSP".to_owned()), self.is_dsp_enabled, |v| {
                Message::SetDspEnabled(v)
            }),
            self.flow_graph_view
                .view()
                .map(Message::FlowGraphViewMessage)
        ]
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
