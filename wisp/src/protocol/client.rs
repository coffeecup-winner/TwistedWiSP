use std::thread::JoinHandle;

use crate::{
    ir::{CallId, IRFunction},
    protocol::{
        CommandResponse, DataIndex, SystemInfo, WatchIndex, WatchedDataValues, WispCommand,
        WispCommandResponse,
    },
};

pub struct WispRunnerClient {
    wisp_thread: JoinHandle<()>,
    command_sender: crossbeam::channel::Sender<WispCommand>,
    response_receiver: crossbeam::channel::Receiver<WispCommandResponse>,
}

impl WispRunnerClient {
    pub fn init(
        preferred_buffer_size: Option<u32>,
        preferred_sample_rate: Option<u32>,
        midi_in_port: Option<&str>,
    ) -> WispRunnerClient {
        let mut log_builder = env_logger::Builder::from_default_env();
        log_builder.target(env_logger::Target::Stderr);
        log_builder.init();

        let args = crate::runner::main::Args {
            list_audio_devices: false,
            audio_host: None,
            audio_device: None,
            audio_output_channels: None,
            audio_buffer_size: preferred_buffer_size,
            audio_sample_rate: preferred_sample_rate,
            midi_in_port: midi_in_port.map(|s| s.to_owned()),
            server: true,
        };
        let (command_sender, command_receiver) = crossbeam::channel::bounded(0);
        let (response_sender, response_receiver) = crossbeam::channel::bounded(0);
        let thread = std::thread::spawn(move || {
            crate::runner::main::main(args, command_receiver, response_sender)
                .expect("Failed to start the server");
        });

        WispRunnerClient {
            wisp_thread: thread,
            command_sender,
            response_receiver,
        }
    }

    pub fn deinit(mut self) {
        self.execute_command(WispCommand::Exit);
        self.wisp_thread
            .join()
            .expect("Failed to join the WiSP thread");
    }

    fn execute_command(&mut self, command: WispCommand) -> CommandResponse {
        self.command_sender
            .send(command)
            .expect("Failed to send a command");
        self.response_receiver
            .recv()
            .expect("Failed to receive the response")
            .unwrap()
    }

    pub fn get_system_info(&mut self) -> SystemInfo {
        if let CommandResponse::SystemInfo(info) = self.execute_command(WispCommand::GetSystemInfo)
        {
            info
        } else {
            panic!("Unexpected response")
        }
    }

    pub fn dsp_start(&mut self) {
        if !matches!(
            self.execute_command(WispCommand::DspStart),
            CommandResponse::Ack
        ) {
            panic!("Unexpected response")
        }
    }

    pub fn dsp_stop(&mut self) {
        if !matches!(
            self.execute_command(WispCommand::DspStop),
            CommandResponse::Ack
        ) {
            panic!("Unexpected response")
        }
    }

    pub fn context_reset(&mut self) {
        if !matches!(
            self.execute_command(WispCommand::ContextReset),
            CommandResponse::Ack
        ) {
            panic!("Unexpected response")
        }
    }

    pub fn context_add_or_update_functions(&mut self, functions: Vec<IRFunction>) {
        if !matches!(
            self.execute_command(WispCommand::ContextAddOrUpdateFunctions(functions)),
            CommandResponse::Ack
        ) {
            panic!("Unexpected response")
        }
    }

    pub fn context_remove_function(&mut self, name: String) {
        if !matches!(
            self.execute_command(WispCommand::ContextRemoveFunction(name)),
            CommandResponse::Ack
        ) {
            panic!("Unexpected response")
        }
    }

    pub fn context_set_main_function(&mut self, name: String) {
        if !matches!(
            self.execute_command(WispCommand::ContextSetMainFunction(name)),
            CommandResponse::Ack
        ) {
            panic!("Unexpected response")
        }
    }

    pub fn context_set_data_value(&mut self, name: String, id: CallId, idx: DataIndex, value: f32) {
        if !matches!(
            self.execute_command(WispCommand::ContextSetDataValue(name, id, idx, value)),
            CommandResponse::Ack
        ) {
            panic!("Unexpected response")
        }
    }

    pub fn context_set_data_array(
        &mut self,
        name: String,
        id: CallId,
        idx: DataIndex,
        array_name: String,
    ) {
        if !matches!(
            self.execute_command(WispCommand::ContextSetDataArray(name, id, idx, array_name)),
            CommandResponse::Ack
        ) {
            panic!("Unexpected response")
        }
    }

    pub fn context_learn_midi_cc(
        &mut self,
        name: String,
        id: CallId,
        idx: DataIndex,
    ) -> Option<WatchIndex> {
        if let CommandResponse::WatchIndex(idx) =
            self.execute_command(WispCommand::ContextLearnMidiCC(name, id, idx))
        {
            idx
        } else {
            panic!("Unexpected response")
        }
    }

    pub fn context_watch_data_value(
        &mut self,
        name: String,
        id: CallId,
        idx: DataIndex,
    ) -> Option<WatchIndex> {
        if let CommandResponse::WatchIndex(idx) =
            self.execute_command(WispCommand::ContextWatchDataValue(name, id, idx))
        {
            idx
        } else {
            panic!("Unexpected response")
        }
    }

    pub fn context_unwatch_data_value(&mut self, idx: WatchIndex) {
        if !matches!(
            self.execute_command(WispCommand::ContextUnwatchDataValue(idx)),
            CommandResponse::Ack
        ) {
            panic!("Unexpected response")
        }
    }

    pub fn context_query_watched_data_values(&mut self) -> WatchedDataValues {
        if let CommandResponse::WatchedDataValues(values) =
            self.execute_command(WispCommand::ContextQueryWatchedDataValues)
        {
            values
        } else {
            panic!("Unexpected response")
        }
    }

    pub fn context_load_wave_file(&mut self, name: String, buffer_name: String, path: String) {
        if !matches!(
            self.execute_command(WispCommand::ContextLoadWaveFile(name, buffer_name, path)),
            CommandResponse::Ack
        ) {
            panic!("Unexpected response")
        }
    }

    pub fn context_unload_wave_file(&mut self, name: String, buffer_name: String) {
        if !matches!(
            self.execute_command(WispCommand::ContextUnloadWaveFile(name, buffer_name)),
            CommandResponse::Ack
        ) {
            panic!("Unexpected response")
        }
    }

    pub fn context_update(&mut self) {
        if !matches!(
            self.execute_command(WispCommand::ContextUpdate),
            CommandResponse::Ack
        ) {
            panic!("Unexpected response")
        }
    }
}
