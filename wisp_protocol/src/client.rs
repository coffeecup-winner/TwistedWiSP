use std::{
    fs::File,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
};

use crate::{
    CommandResponse, FlowNodeIndex, FlowNodeInletIndex, FlowNodeOutletIndex, WispCommand,
    WispCommandResponse,
};

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

    pub fn enable_dsp(&mut self) {
        self.execute_command(WispCommand::StartDsp)
    }

    pub fn disable_dsp(&mut self) {
        self.execute_command(WispCommand::StopDsp)
    }

    pub fn create_function(&mut self) -> String {
        self.execute_command::<String>(WispCommand::CreateFunction)
    }

    pub fn remove_function(&mut self, name: String) {
        self.execute_command(WispCommand::RemoveFunction(name))
    }

    pub fn flow_add_node(&mut self, flow_name: String, func_name: String) -> FlowNodeIndex {
        self.execute_command(WispCommand::FlowAddNode(flow_name, func_name))
    }

    pub fn flow_connect(
        &mut self,
        flow_name: String,
        node_out: FlowNodeIndex,
        node_outlet: FlowNodeOutletIndex,
        node_in: FlowNodeIndex,
        node_inlet: FlowNodeInletIndex,
    ) {
        self.execute_command(WispCommand::FlowConnect(
            flow_name,
            node_out,
            node_outlet,
            node_in,
            node_inlet,
        ))
    }

    pub fn flow_disconnect(
        &mut self,
        flow_name: String,
        node_out: FlowNodeIndex,
        node_outlet: FlowNodeOutletIndex,
        node_in: FlowNodeIndex,
        node_inlet: FlowNodeInletIndex,
    ) {
        self.execute_command(WispCommand::FlowDisconnect(
            flow_name,
            node_out,
            node_outlet,
            node_in,
            node_inlet,
        ))
    }

    pub fn deinit(mut self) {
        self.execute_command::<()>(WispCommand::Exit);
        self.wisp_process
            .wait_with_output()
            .expect("Failed to stop the WiSP process");
    }
}
