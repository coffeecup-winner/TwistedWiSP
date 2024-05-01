use serde::{de::DeserializeOwned, Deserialize, Serialize};
use twisted_wisp_ir::{CallId, IRFunction};

#[derive(Debug, Serialize, Deserialize)]
pub struct SystemInfo {
    pub num_channels: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DataIndex(pub u32);

#[derive(Debug, Serialize, Deserialize)]
pub enum WispCommand {
    // System commands
    GetSystemInfo, // -> SystemInfo
    DspStart,
    DspStop,
    Exit,

    // Context commands
    ContextReset,
    ContextAddOrUpdateFunction(IRFunction),
    ContextRemoveFunction(String),
    ContextSetMainFunction(String),
    ContextSetDataValue(String, CallId, DataIndex, f32),
    ContextUpdate,
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
pub enum WispCommandResponse<T> {
    Ok(T),
    NonFatalFailure,
    FatalFailure,
}

pub trait CommandResponse: Serialize + DeserializeOwned {}
impl CommandResponse for () {}
impl CommandResponse for SystemInfo {}

impl<T> WispCommandResponse<T>
where
    T: CommandResponse,
{
    pub fn from_json(json: &str) -> WispCommandResponse<T> {
        serde_json::from_str(json).expect("Failed to deserialize a WiSP command response")
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("Failed to serialize a WiSP command response")
    }

    pub fn unwrap(self) -> T {
        match self {
            WispCommandResponse::Ok(v) => v,
            WispCommandResponse::NonFatalFailure => {
                // TODO: Return Result instead
                panic!("Non-fatal failure happened")
            }
            WispCommandResponse::FatalFailure => panic!("Fatal failure happened"),
        }
    }
}
