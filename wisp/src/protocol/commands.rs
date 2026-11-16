use std::collections::HashMap;

use crate::ir::{CallId, IRFunction};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct SystemInfo {
    pub num_channels: u32,
}

#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Serialize, Deserialize)]
pub struct DataIndex(pub u32);

#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub struct WatchIndex(pub u32);

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct WatchedDataValues {
    pub values: HashMap<WatchIndex, Vec<f32>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum WispCommand {
    // System commands
    GetSystemInfo, // -> SystemInfo
    DspStart,
    DspStop,
    Exit,

    // Context commands
    ContextReset,
    ContextAddOrUpdateFunctions(Vec<IRFunction>),
    ContextRemoveFunction(String),
    ContextSetMainFunction(String),
    ContextSetDataValue(String, CallId, DataIndex, f32),
    ContextSetDataArray(String, CallId, DataIndex, String),
    ContextLearnMidiCC(String, CallId, DataIndex), // -> Option<WatchIndex>
    ContextWatchDataValue(String, CallId, DataIndex), // -> Option<WatchIndex>
    ContextUnwatchDataValue(WatchIndex),
    ContextQueryWatchedDataValues, // -> WatchedDataValues
    ContextLoadWaveFile(String, String, String),
    ContextUnloadWaveFile(String, String),
    ContextUpdate,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum WispCommandResponse {
    Ok(CommandResponse),
    NonFatalFailure,
    FatalFailure,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum CommandResponse {
    Ack,
    SystemInfo(SystemInfo),
    WatchIndex(Option<WatchIndex>),
    WatchedDataValues(WatchedDataValues),
}

impl WispCommandResponse {
    pub fn unwrap(self) -> CommandResponse {
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
