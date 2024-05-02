use std::{
    fs::File,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
};

use twisted_wisp_ir::{CallId, IRFunction};

use crate::{
    CommandResponse, DataIndex, SystemInfo, WatchIndex, WatchedDataValues, WispCommand,
    WispCommandResponse,
};

pub struct WispRunnerClient {
    wisp_process: Child,
}

impl WispRunnerClient {
    pub fn init(
        exe_path: &Path,
        preferred_buffer_size: Option<u32>,
        preferred_sample_rate: Option<u32>,
    ) -> WispRunnerClient {
        let log_file = File::create("wisp.log").expect("Failed to create the log file");
        let mut command = Command::new(exe_path);
        if let Some(buffer_size) = preferred_buffer_size {
            command.args(["--audio-buffer-size", &buffer_size.to_string()]);
        }
        if let Some(sample_rate) = preferred_sample_rate {
            command.args(["--audio-sample-rate", &sample_rate.to_string()]);
        }
        let child = command
            .arg("--server")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(log_file))
            .spawn()
            .expect("Failed to start the client");
        WispRunnerClient {
            wisp_process: child,
        }
    }

    pub fn deinit(mut self) {
        self.execute_command::<()>(WispCommand::Exit);
        self.wisp_process
            .wait_with_output()
            .expect("Failed to stop the WiSP process");
    }

    fn execute_command<T>(&mut self, command: WispCommand) -> T
    where
        T: CommandResponse,
    {
        let mut command = command.to_json();
        command.push('\n');
        self.wisp_process
            .stdin
            .as_ref()
            .unwrap()
            .write_all(command.as_bytes())
            .expect("Failed to run a command");
        let mut reader = BufReader::new(self.wisp_process.stdout.as_mut().unwrap());
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .expect("Failed to receive the response");
        WispCommandResponse::<T>::from_json(&line).unwrap()
    }

    pub fn get_system_info(&mut self) -> SystemInfo {
        self.execute_command(WispCommand::GetSystemInfo)
    }

    pub fn dsp_start(&mut self) {
        self.execute_command(WispCommand::DspStart)
    }

    pub fn dsp_stop(&mut self) {
        self.execute_command(WispCommand::DspStop)
    }

    pub fn context_reset(&mut self) {
        self.execute_command(WispCommand::ContextReset)
    }

    pub fn context_add_or_update_function(&mut self, func: IRFunction) {
        self.execute_command(WispCommand::ContextAddOrUpdateFunction(func))
    }

    pub fn context_remove_function(&mut self, name: String) {
        self.execute_command(WispCommand::ContextRemoveFunction(name))
    }

    pub fn context_set_main_function(&mut self, name: String) {
        self.execute_command(WispCommand::ContextSetMainFunction(name))
    }

    pub fn context_set_data_value(&mut self, name: String, id: CallId, idx: DataIndex, value: f32) {
        self.execute_command(WispCommand::ContextSetDataValue(name, id, idx, value))
    }

    pub fn context_watch_data_value(
        &mut self,
        name: String,
        id: CallId,
        idx: DataIndex,
    ) -> Option<WatchIndex> {
        self.execute_command(WispCommand::ContextWatchDataValue(name, id, idx))
    }

    pub fn context_unwatch_data_value(&mut self, idx: WatchIndex) {
        self.execute_command(WispCommand::ContextUnwatchDataValue(idx))
    }

    pub fn context_query_watched_data_values(&mut self) -> WatchedDataValues {
        self.execute_command(WispCommand::ContextQueryWatchedDataValues)
    }

    pub fn context_update(&mut self) {
        self.execute_command(WispCommand::ContextUpdate)
    }
}
