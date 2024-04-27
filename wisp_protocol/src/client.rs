use std::{
    fs::File,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
};

use twisted_wisp_ir::IRFunction;

use crate::{CommandResponse, SystemInfo, WispCommand, WispCommandResponse};

pub struct WispRunnerClient {
    wisp_process: Child,
}

impl WispRunnerClient {
    pub fn init(exe_path: &Path) -> WispRunnerClient {
        let log_file = File::create("wisp.log").expect("Failed to create the log file");
        let child = Command::new(exe_path)
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

    pub fn context_update(&mut self) {
        self.execute_command(WispCommand::ContextUpdate)
    }
}
