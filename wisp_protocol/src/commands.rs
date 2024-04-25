use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WispCommand {
    StartDsp,
    StopDsp,
    Exit,
}

impl WispCommand {
    pub fn from_json(json: &str) -> WispCommand {
        serde_json::from_str(json).expect("Failed to deserialize a WiSP command")
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("Failed to serialize a WiSP command")
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub enum WispCommandResponse {
    Ok,
    NonFatalFailure,
    FatalFailure,
}

impl WispCommandResponse {
    pub fn from_json(json: &str) -> WispCommandResponse {
        serde_json::from_str(json).expect("Failed to deserialize a WiSP command response")
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("Failed to serialize a WiSP command response")
    }
}
