use std::{
    fs::File,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
};

use crate::{WispCommand, WispCommandResponse};

pub struct WispClient {
    wisp_process: Child,
}

impl WispClient {
    pub fn init(exe_path: &Path) -> WispClient {
        let log_file = File::create("wisp.log").expect("Failed to create the log file");
        let child = Command::new(exe_path)
            .arg("--server")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(log_file))
            .spawn()
            .expect("Failed to start the client");
        WispClient {
            wisp_process: child,
        }
    }

    fn execute_command(&mut self, command: WispCommand) -> WispCommandResponse {
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
        WispCommandResponse::from_json(&line)
    }

    pub fn enable_dsp(&mut self) {
        let resp = self.execute_command(WispCommand::StartDsp);
        assert!(resp == WispCommandResponse::Ok);
    }

    pub fn disable_dsp(&mut self) {
        let resp = self.execute_command(WispCommand::StopDsp);
        assert!(resp == WispCommandResponse::Ok);
    }

    pub fn deinit(mut self) {
        self.execute_command(WispCommand::Exit);
        self.wisp_process
            .wait_with_output()
            .expect("Failed to stop the WiSP process");
    }
}
